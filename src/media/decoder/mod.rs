//! In-process decoders. The viewer sees only oriented sRGB pixels, never codec types.
mod jpeg;
mod raw;

use image::{DynamicImage, ImageDecoder, ImageReader, RgbaImage, metadata::Orientation};
use moxcms::{ColorProfile, DataColorSpace, Layout};
use std::{
    fs::File,
    io::{BufReader, Cursor, Read},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

pub type Result<T> = std::result::Result<T, String>;
pub const MAX_PIXELS: u64 = 120_000_000;
const MAX_INPUT: u64 = 512 * 1024 * 1024;

#[derive(Clone)]
pub struct Cancellation {
    pub generation: Arc<AtomicU64>,
    pub id: u64,
}
impl Cancellation {
    pub fn cancelled(&self) -> bool {
        self.generation.load(Ordering::Relaxed) != self.id
    }
    pub fn check(&self) -> Result<()> {
        if self.cancelled() {
            Err("Cancelled".into())
        } else {
            Ok(())
        }
    }
}

pub struct DecodeRequest<'a> {
    pub path: &'a Path,
    /// Content-derived ExifTool FileType, never the filename extension.
    pub format: &'a str,
    pub orientation: u8,
    pub full: bool,
    pub edge: u32,
}
pub struct Decoded {
    pub pixels: RgbaImage,
    pub original: (u32, u32),
    pub preview: bool,
    pub note: String,
}
trait Decoder {
    fn decode(&self, request: &DecodeRequest<'_>, cancel: &Cancellation) -> Result<Decoded>;
}

pub fn decode(request: &DecodeRequest<'_>, cancel: &Cancellation) -> Result<Decoded> {
    decode_progressive(request, cancel, None)
}

pub fn decode_progressive(
    request: &DecodeRequest<'_>,
    cancel: &Cancellation,
    preview_ready: Option<&mut dyn FnMut(Decoded)>,
) -> Result<Decoded> {
    cancel.check()?;
    let size = std::fs::metadata(request.path).map_err(err)?.len();
    if size > MAX_INPUT {
        return Err("File exceeds the 512 MiB input limit".into());
    }
    // RAW routing precedes generic TIFF and ISO-BMFF (CR3) decoding.
    if is_raw(request.format) {
        let result = raw::RawDecoder.decode_progressive(request, cancel, preview_ready)?;
        cancel.check()?;
        return Ok(result);
    }
    let decoder: &dyn Decoder = if matches!(request.format, "HEIC" | "HEIF" | "HIF" | "AVIF") {
        &HeifDecoder
    } else {
        &RasterDecoder
    };
    let result = decoder.decode(request, cancel)?;
    cancel.check()?;
    Ok(result)
}

pub fn is_raw(format: &str) -> bool {
    matches!(
        format,
        "3FR"
            | "ARW"
            | "CR2"
            | "CR3"
            | "CRW"
            | "DCR"
            | "DNG"
            | "ERF"
            | "FFF"
            | "IIQ"
            | "K25"
            | "KDC"
            | "MEF"
            | "MOS"
            | "MRW"
            | "NEF"
            | "NRW"
            | "ORF"
            | "PEF"
            | "RAF"
            | "RAW"
            | "RW2"
            | "RWL"
            | "SR2"
            | "SRF"
            | "SRW"
            | "X3F"
    )
}
pub(super) fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub(super) fn check_size(w: u32, h: u32) -> Result<()> {
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(format!(
            "Image dimensions {w} × {h} exceed the 120 megapixel viewing limit"
        ));
    }
    Ok(())
}
pub(super) fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(err)?
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err("File exceeds the 512 MiB input limit".into());
    }
    Ok(bytes)
}

struct RasterDecoder;
impl Decoder for RasterDecoder {
    fn decode(&self, request: &DecodeRequest<'_>, cancel: &Cancellation) -> Result<Decoded> {
        if request.format == "JPEG" {
            let bytes = read_bytes(request.path)?;
            if let Some(result) = jpeg::decode(&bytes, request, cancel)? {
                return Ok(result);
            }
        }
        let file = BufReader::new(File::open(request.path).map_err(err)?);
        if matches!(request.format, "JXL" | "JXL Codestream") {
            return decode_image(
                jxl_oxide::integration::JxlDecoder::new(file).map_err(err)?,
                request,
                cancel,
                true,
            );
        }
        let reader = ImageReader::new(file).with_guessed_format().map_err(err)?;
        decode_image(reader.into_decoder().map_err(err)?, request, cancel, false)
    }
}

fn decode_image(
    mut decoder: impl ImageDecoder,
    request: &DecodeRequest<'_>,
    cancel: &Cancellation,
    oriented: bool,
) -> Result<Decoded> {
    let (w, h) = decoder.dimensions();
    check_size(w, h)?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(768 * 1024 * 1024);
    decoder.set_limits(limits).map_err(err)?;
    let icc = decoder.icc_profile().map_err(err)?;
    let orientation = if oriented {
        Orientation::NoTransforms
    } else {
        let own = decoder.orientation().map_err(err)?;
        if own == Orientation::NoTransforms {
            Orientation::from_exif(request.orientation).unwrap_or(own)
        } else {
            own
        }
    };
    let mut img = DynamicImage::from_decoder(decoder).map_err(err)?;
    cancel.check()?;
    let original = if matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    ) {
        (h, w)
    } else {
        (w, h)
    };
    // Shrink before rotating: moving all 26+ million pixels is wasted work for a preview.
    img = resize_for_view(img, request)?;
    cancel.check()?;
    img.apply_orientation(orientation);
    let profile = icc.as_deref().map(ColorProfile::new_from_slice).transpose();
    let mut result = finish(img, profile.map_err(err), request, cancel, false, "")?;
    result.original = original;
    Ok(result)
}

pub(super) fn finish(
    img: DynamicImage,
    profile: Result<Option<ColorProfile>>,
    request: &DecodeRequest<'_>,
    cancel: &Cancellation,
    preview: bool,
    note: &str,
) -> Result<Decoded> {
    check_size(img.width(), img.height())?;
    let original = (img.width(), img.height());
    let img = resize_for_view(img, request)?;
    cancel.check()?;
    let mut notes = note.to_owned();
    let pixels = match profile.and_then(|p| match p {
        Some(profile) => convert_color(&img, &profile),
        None => Ok(img.to_rgba8()),
    }) {
        Ok(pixels) => pixels,
        Err(error) => {
            if !notes.is_empty() {
                notes.push_str(" · ");
            }
            notes.push_str(&format!("Colour profile unavailable: {error}"));
            img.to_rgba8()
        }
    };
    Ok(Decoded {
        pixels,
        original,
        preview,
        note: notes,
    })
}

fn fit_dimensions(width: u32, height: u32, edge: u32) -> (u32, u32) {
    if width.max(height) <= edge {
        return (width, height);
    }
    let ratio = f64::from(edge.max(1)) / f64::from(width.max(height));
    (
        (f64::from(width) * ratio).round().max(1.0) as u32,
        (f64::from(height) * ratio).round().max(1.0) as u32,
    )
}

fn resize_for_view(img: DynamicImage, request: &DecodeRequest<'_>) -> Result<DynamicImage> {
    if request.full {
        return Ok(img);
    }
    let (width, height) = fit_dimensions(img.width(), img.height(), request.edge);
    resize_image(img, width, height)
}

fn resize_image(img: DynamicImage, width: u32, height: u32) -> Result<DynamicImage> {
    if (img.width(), img.height()) == (width, height) {
        return Ok(img);
    }
    let mut output = DynamicImage::new(width, height, img.color());
    let options = fast_image_resize::ResizeOptions::new().resize_alg(
        fast_image_resize::ResizeAlg::Convolution(fast_image_resize::FilterType::Bilinear),
    );
    fast_image_resize::Resizer::new()
        .resize(&img, &mut output, &options)
        .map_err(err)?;
    Ok(output)
}

fn convert_color(img: &DynamicImage, profile: &ColorProfile) -> Result<RgbaImage> {
    let (layout, input) = match profile.color_space {
        DataColorSpace::Rgb => (Layout::Rgba, img.to_rgba8().into_raw()),
        DataColorSpace::Gray => (Layout::GrayAlpha, img.to_luma_alpha8().into_raw()),
        _ => return Err("unsupported profile colour space".into()),
    };
    let transform = profile
        .create_transform_8bit(
            layout,
            &ColorProfile::new_srgb(),
            Layout::Rgba,
            Default::default(),
        )
        .map_err(err)?;
    let mut output = vec![0; img.width() as usize * img.height() as usize * 4];
    transform.transform(&input, &mut output).map_err(err)?;
    RgbaImage::from_raw(img.width(), img.height(), output)
        .ok_or_else(|| "Invalid colour output".into())
}

struct HeifDecoder;
impl Decoder for HeifDecoder {
    fn decode(&self, request: &DecodeRequest<'_>, cancel: &Cancellation) -> Result<Decoded> {
        use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};
        let bytes = read_bytes(request.path)?;
        let lib = LibHeif::new();
        let mut ctx = HeifContext::read_from_bytes(&bytes).map_err(err)?;
        ctx.set_max_decoding_threads(2);
        let handle = ctx.primary_image_handle().map_err(err)?;
        check_size(handle.width(), handle.height())?;
        let icc = handle.color_profile_raw();
        let nclx = handle.color_profile_nclx();
        let profile = if let Some(icc) = icc {
            ColorProfile::new_from_slice(&icc.data)
                .map(Some)
                .map_err(err)
        } else if let Some(nclx) = nclx {
            (|| {
                let primaries = moxcms::CicpColorPrimaries::try_from(nclx.color_primaries() as u8)
                    .map_err(err)?;
                let transfer = moxcms::TransferCharacteristics::try_from(
                    nclx.transfer_characteristics() as u8,
                )
                .map_err(err)?;
                // Validate before constructing: moxcms 0.8.1's update method
                // returns false even on success, so its boolean is not a validity check.
                moxcms::ColorPrimaries::try_from(primaries).map_err(err)?;
                moxcms::ToneReprCurve::try_from(transfer).map_err(err)?;
                let profile = ColorProfile::new_from_cicp(moxcms::CicpProfile {
                    color_primaries: primaries,
                    transfer_characteristics: transfer,
                    matrix_coefficients: moxcms::MatrixCoefficients::Identity,
                    full_range: true,
                });
                Ok(Some(profile))
            })()
        } else {
            Ok(None)
        };
        // libheif applies the container's crop, rotation and mirroring. Do not apply EXIF again.
        let decoded = lib
            .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgba), None)
            .map_err(err)?;
        cancel.check()?;
        check_size(decoded.width(), decoded.height())?;
        let planes = decoded.planes();
        let plane = planes
            .interleaved
            .ok_or("HEIF decoder returned no pixels")?;
        let row = decoded.width() as usize * 4;
        let mut data = Vec::with_capacity(row * decoded.height() as usize);
        for y in 0..decoded.height() as usize {
            let start = y * plane.stride;
            data.extend_from_slice(
                plane
                    .data
                    .get(start..start + row)
                    .ok_or("Invalid HEIF stride")?,
            );
        }
        let img = RgbaImage::from_raw(decoded.width(), decoded.height(), data)
            .ok_or("Invalid HEIF pixels")?;
        finish(
            DynamicImage::ImageRgba8(img),
            profile,
            request,
            cancel,
            false,
            "",
        )
    }
}

pub(super) fn preview_jpeg(
    bytes: &[u8],
    request: &DecodeRequest<'_>,
    cancel: &Cancellation,
) -> Result<Decoded> {
    let mut result = if let Some(result) = jpeg::decode(bytes, request, cancel)? {
        result
    } else {
        let decoder = image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes)).map_err(err)?;
        decode_image(decoder, request, cancel, false)?
    };
    result.preview = true;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn token() -> Cancellation {
        Cancellation {
            generation: Arc::new(AtomicU64::new(1)),
            id: 1,
        }
    }

    /// Repeatable development-build benchmark; reads a local photo without changing it.
    #[test]
    #[ignore = "JPEG benchmark; optionally override PHOTO_BENCH_FILE"]
    fn profile_jpeg() {
        use std::time::Instant;
        let path = std::env::var_os("PHOTO_BENCH_FILE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| crate::test_image("common/sample.jpg"));
        for _ in 0..3 {
            let request = DecodeRequest {
                path: &path,
                format: "JPEG",
                orientation: 1,
                full: false,
                edge: 1600,
            };
            let start = Instant::now();
            let actual = decode(&request, &token()).unwrap();
            eprintln!(
                "production JPEG preview total={:?} output={:?}",
                start.elapsed(),
                actual.pixels.dimensions()
            );
            let start = Instant::now();
            let mut decoder = ImageReader::open(&path)
                .unwrap()
                .with_guessed_format()
                .unwrap()
                .into_decoder()
                .unwrap();
            let icc = decoder.icc_profile().unwrap();
            let orientation = decoder.orientation().unwrap();
            let header = start.elapsed();
            let mut img = DynamicImage::from_decoder(decoder).unwrap();
            let decode = start.elapsed();
            let request = DecodeRequest {
                path: &path,
                format: "JPEG",
                orientation: 1,
                full: false,
                edge: 1600,
            };
            img = resize_for_view(img, &request).unwrap();
            let resize = start.elapsed();
            img.apply_orientation(orientation);
            let orient = start.elapsed();
            let result = finish(
                img,
                icc.as_deref()
                    .map(ColorProfile::new_from_slice)
                    .transpose()
                    .map_err(err),
                &request,
                &token(),
                false,
                "",
            )
            .unwrap();
            eprintln!(
                "header={header:?} decode={:?} resize={:?} orientation={:?} colour={:?} total={:?} output={:?}",
                decode - header,
                resize - decode,
                orient - resize,
                start.elapsed() - orient,
                start.elapsed(),
                result.pixels.dimensions()
            );
        }
    }

    #[test]
    fn common_formats_detect_content_and_apply_orientation_once() {
        let dir = tempfile::tempdir().unwrap();
        for (sample, name) in [
            ("common/sample.png", "PNG"),
            ("common/sample.jpg", "JPEG"),
            ("common/sample.tiff", "TIFF"),
            ("web/sample.webp", "WEBP"),
            ("common/sample.bmp", "BMP"),
            ("common/sample.gif", "GIF"),
        ] {
            let path = dir.path().join(format!("{name}.wrong"));
            std::fs::copy(crate::test_image(sample), &path).unwrap();
            let mut request = DecodeRequest {
                path: &path,
                format: name,
                orientation: 6,
                full: true,
                edge: 4,
            };
            let decoded = decode(&request, &token()).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(decoded.pixels.dimensions(), (64, 64), "{name}");
            request.orientation = 1;
            let upright = decode(&request, &token()).unwrap();
            assert_eq!(
                decoded.pixels,
                image::imageops::rotate90(&upright.pixels),
                "{name}"
            );
            request.orientation = 6;
            request.full = false;
            assert_eq!(
                decode(&request, &token()).unwrap().pixels.dimensions(),
                (4, 4)
            );
        }
    }

    #[test]
    fn preview_preserves_original_dimensions_for_all_exif_orientations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orientation.png");
        image::open(crate::test_image("common/sample.png"))
            .unwrap()
            .resize_exact(120, 80, image::imageops::FilterType::Nearest)
            .save(&path)
            .unwrap();
        for orientation in 1..=8 {
            let mut request = DecodeRequest {
                path: &path,
                format: "PNG",
                orientation,
                full: false,
                edge: 40,
            };
            let preview = decode(&request, &token()).unwrap();
            request.full = true;
            let full = decode(&request, &token()).unwrap();
            let swapped = orientation >= 5;
            assert_eq!(
                preview.original,
                if swapped { (80, 120) } else { (120, 80) }
            );
            assert_eq!(preview.original, full.pixels.dimensions());
            assert_eq!(
                preview.pixels.dimensions(),
                if swapped { (27, 40) } else { (40, 27) }
            );
        }
    }

    #[test]
    fn scaled_jpeg_keeps_original_aspect_and_full_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("odd.jpg");
        image::open(crate::test_image("common/sample.jpg"))
            .unwrap()
            .resize_exact(513, 257, image::imageops::FilterType::Nearest)
            .save(&path)
            .unwrap();
        for orientation in [1, 6] {
            for edge in [64, 128, 256, 512, 1024] {
                let mut request = DecodeRequest {
                    path: &path,
                    format: "JPEG",
                    orientation,
                    full: false,
                    edge,
                };
                let preview = decode(&request, &token()).unwrap();
                let (w, h) = fit_dimensions(513, 257, edge);
                assert_eq!(
                    preview.pixels.dimensions(),
                    if orientation == 6 { (h, w) } else { (w, h) }
                );
                request.full = true;
                let full = decode(&request, &token()).unwrap();
                assert_eq!(
                    full.pixels.dimensions(),
                    if orientation == 6 {
                        (257, 513)
                    } else {
                        (513, 257)
                    }
                );
                assert_eq!(preview.original, full.pixels.dimensions());
            }
        }
    }

    #[test]
    fn avif_and_jxl_decode_colour_and_renamed_files() {
        let dir = tempfile::tempdir().unwrap();
        let reference = image::open(crate::test_image("common/sample.png"))
            .unwrap()
            .to_rgba8();
        for (format, sample) in [("AVIF", "web/sample.avif"), ("JXL", "web/sample.jxl")] {
            let path = dir.path().join(format!("{format}.data"));
            std::fs::copy(crate::test_image(sample), &path).unwrap();
            let request = DecodeRequest {
                path: &path,
                format,
                orientation: 1,
                full: true,
                edge: 32,
            };
            let decoded = decode(&request, &token()).unwrap();
            assert_eq!(decoded.pixels.dimensions(), (64, 64));
            assert!(decoded.note.is_empty(), "{}", decoded.note);
            for (x, y) in [(16, 16), (32, 32), (48, 48)] {
                let actual = decoded.pixels.get_pixel(x, y);
                let expected = reference.get_pixel(x, y);
                for (a, b) in actual.0.into_iter().zip(expected.0) {
                    assert!(
                        a.abs_diff(b) <= 16,
                        "{format} ({x},{y}): {actual:?} != {expected:?}"
                    );
                }
            }
            // The lossy samples can round transparent alpha by one level.
            assert!(decoded.pixels.get_pixel(0, 0)[3] <= 2, "{format}");
        }
        // Exercise ExifTool's actual FileType labels (including "JXL Codestream").
        let report = crate::media::scan(dir.path()).unwrap();
        assert_eq!(report.images.len(), 2);
        for entry in report.images {
            let request = DecodeRequest {
                path: &entry.path,
                format: &entry.format,
                orientation: entry.orientation,
                full: false,
                edge: 32,
            };
            assert_eq!(decode(&request, &token()).unwrap().pixels.width(), 32);
        }
    }

    #[test]
    fn corrupt_inputs_and_cancelled_requests_fail_without_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken");
        std::fs::write(&path, b"not an image").unwrap();
        for format in ["JPEG", "RAF", "HEIC", "JXL"] {
            let request = DecodeRequest {
                path: &path,
                format,
                orientation: 1,
                full: false,
                edge: 100,
            };
            assert!(decode(&request, &token()).is_err(), "{format}");
        }
        let cancel = token();
        cancel.generation.store(2, Ordering::Relaxed);
        assert!(cancel.check().is_err());
        assert!(check_size(20_000, 20_000).is_err());
        assert!(is_raw("DNG"));
        assert!(!is_raw("TIFF"));
    }

    #[test]
    fn icc_transform_preserves_srgb_and_alpha() {
        let img =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(3, 2, image::Rgba([110, 50, 220, 80])));
        let result = convert_color(&img, &ColorProfile::new_srgb()).unwrap();
        for pixel in result.pixels() {
            for (actual, expected) in pixel.0.into_iter().zip([110u8, 50, 220, 80]) {
                assert!(actual.abs_diff(expected) <= 1, "{pixel:?}");
            }
        }
    }

    /// Opt-in, read-only smoke test; defaults to the bundled Nikon RAW sample.
    #[test]
    #[ignore = "RAW development smoke test; optionally override PHOTO_SAMPLE_DIR"]
    fn local_camera_samples() {
        let folder = std::env::var_os("PHOTO_SAMPLE_DIR");
        let report = crate::media::scan(
            &folder
                .as_ref()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| crate::test_image("raw")),
        )
        .unwrap();
        let mut seen = std::collections::HashSet::new();
        for entry in report.images {
            if folder.is_none() && entry.path != crate::test_image("raw/Nikon_D2H.nef") {
                continue;
            }
            if !seen.insert(entry.format.clone()) {
                continue;
            }
            for full in [false, true] {
                let request = DecodeRequest {
                    path: &entry.path,
                    format: &entry.format,
                    orientation: entry.orientation,
                    full,
                    edge: 1200,
                };
                let start = std::time::Instant::now();
                let decoded = decode(&request, &token())
                    .unwrap_or_else(|e| panic!("{} full={full}: {e}", entry.path.display()));
                assert!(decoded.pixels.width() > 0);
                assert!(!full || !decoded.preview);
                if !full {
                    assert!(decoded.pixels.width().max(decoded.pixels.height()) <= 1200);
                }
                eprintln!(
                    "{} full={full} preview={} source={:?} output={:?} elapsed={:?} {}",
                    entry.format,
                    decoded.preview,
                    decoded.original,
                    decoded.pixels.dimensions(),
                    start.elapsed(),
                    decoded.note
                );
                assert!(!decoded.note.contains("unavailable"), "{}", decoded.note);
            }
        }
        assert!(!seen.is_empty());
    }
}
