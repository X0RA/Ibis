use super::*;
use std::{
    ffi::{CStr, c_char, c_int, c_void},
    ptr,
};

#[repr(C)]
#[derive(Default)]
struct Pixels {
    data: *const u8,
    size: usize,
    width: u32,
    height: u32,
    colors: u32,
    bits: u32,
    jpeg: u32,
    owner: *mut c_void,
}
unsafe extern "C" {
    fn photo_raw_open(
        path: *const c_void,
        half_size: c_int,
        cb: extern "C" fn(*mut c_void) -> c_int,
        context: *mut c_void,
        error: *mut c_int,
    ) -> *mut c_void;
    fn photo_raw_close(raw: *mut c_void);
    fn photo_raw_width(raw: *mut c_void) -> u32;
    fn photo_raw_height(raw: *mut c_void) -> u32;
    fn photo_raw_preview(raw: *mut c_void, out: *mut Pixels) -> c_int;
    fn photo_raw_develop(raw: *mut c_void, out: *mut Pixels) -> c_int;
    fn photo_raw_free(owner: *mut c_void);
    fn photo_raw_error(code: c_int) -> *const c_char;
}
// The callback context outlives the native decoder and its owned file stream.
struct Raw<'a> {
    ptr: *mut c_void,
    _cancel: &'a Cancellation,
}
impl Drop for Raw<'_> {
    fn drop(&mut self) {
        unsafe { photo_raw_close(self.ptr) };
    }
}
impl Drop for Pixels {
    fn drop(&mut self) {
        if !self.owner.is_null() {
            unsafe { photo_raw_free(self.owner) };
        }
    }
}
extern "C" fn cancelled(context: *mut c_void) -> c_int {
    // Installed only while the borrowed Cancellation is alive; LibRaw calls synchronously.
    unsafe { (&*context.cast::<Cancellation>()).cancelled() as c_int }
}
fn check(code: c_int) -> Result<()> {
    if code == 0 {
        return Ok(());
    }
    let message = unsafe { CStr::from_ptr(photo_raw_error(code)) }.to_string_lossy();
    Err(format!("LibRaw: {message}"))
}
impl<'a> Raw<'a> {
    fn open(path: &Path, half_size: bool, cancel: &'a Cancellation) -> Result<Self> {
        cancel.check()?;
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStrExt;
            std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(err)?
        };
        #[cfg(windows)]
        let path = {
            use std::os::windows::ffi::OsStrExt;
            let mut path: Vec<u16> = path.as_os_str().encode_wide().collect();
            if path.contains(&0) {
                return Err("Path contains a NUL".into());
            }
            path.push(0);
            path
        };
        let mut code = 0;
        let ptr = unsafe {
            photo_raw_open(
                path.as_ptr().cast(),
                half_size as c_int,
                cancelled,
                ptr::from_ref(cancel).cast_mut().cast(),
                &mut code,
            )
        };
        check(code)?;
        if ptr.is_null() {
            return Err("LibRaw allocation failed".into());
        }
        Ok(Self {
            ptr,
            _cancel: cancel,
        })
    }
}
impl Pixels {
    fn bytes(&self) -> Result<&[u8]> {
        if self.data.is_null() || self.size > 512 * 1024 * 1024 {
            return Err("Invalid RAW pixel buffer".into());
        }
        // Native allocation remains owned by self and has exactly size accessible bytes.
        Ok(unsafe { std::slice::from_raw_parts(self.data, self.size) })
    }
    fn image(&self) -> Result<DynamicImage> {
        check_size(self.width, self.height)?;
        if self.bits != 8 {
            return Err("Unsupported RAW output bit depth".into());
        }
        let bytes = self.bytes()?.to_vec();
        match self.colors {
            3 => image::RgbImage::from_raw(self.width, self.height, bytes)
                .map(DynamicImage::ImageRgb8),
            1 => image::GrayImage::from_raw(self.width, self.height, bytes)
                .map(DynamicImage::ImageLuma8),
            _ => None,
        }
        .ok_or("Unsupported RAW pixel layout".into())
    }
}
pub(super) struct RawDecoder;
impl Decoder for RawDecoder {
    fn decode(&self, request: &DecodeRequest<'_>, cancel: &Cancellation) -> Result<Decoded> {
        self.decode_progressive(request, cancel, None)
    }
}
impl RawDecoder {
    pub(super) fn decode_progressive(
        &self,
        request: &DecodeRequest<'_>,
        cancel: &Cancellation,
        mut preview_ready: Option<&mut dyn FnMut(Decoded)>,
    ) -> Result<Decoded> {
        let mut raw = Raw::open(request.path, !request.full, cancel)?;
        let mut original = (unsafe { photo_raw_width(raw.ptr) }, unsafe {
            photo_raw_height(raw.ptr)
        });
        check_size(original.0, original.1)?;
        if matches!(request.orientation, 5..=8) {
            original = (original.1, original.0);
        }
        if !request.full || preview_ready.is_some() {
            let mut pixels = Pixels::default();
            let code = unsafe { photo_raw_preview(raw.ptr, &mut pixels) };
            cancel.check()?;
            if code == 0 {
                let preview_request = DecodeRequest {
                    full: false,
                    ..*request
                };
                let preview = if pixels.jpeg != 0 {
                    pixels
                        .bytes()
                        .and_then(|bytes| preview_jpeg(bytes, &preview_request, cancel))
                } else {
                    pixels.image().and_then(|img| {
                        let mut img = resize_for_view(img, &preview_request)?;
                        img.apply_orientation(
                            Orientation::from_exif(request.orientation)
                                .unwrap_or(Orientation::NoTransforms),
                        );
                        finish(img, Ok(None), &preview_request, cancel, true, "")
                    })
                };
                if let Ok(mut preview) = preview {
                    preview.original = original;
                    cancel.check()?;
                    if !request.full {
                        return Ok(preview);
                    }
                    if let Some(callback) = preview_ready.as_mut() {
                        callback(preview);
                    }
                }
            } else {
                // A failed LibRaw thumbnail call can invalidate its context.
                drop(raw);
                raw = Raw::open(request.path, !request.full, cancel)?;
            }
        }
        cancel.check()?;
        let mut pixels = Pixels::default();
        check(unsafe { photo_raw_develop(raw.ptr, &mut pixels) })?;
        cancel.check()?;
        let mut decoded = finish(
            pixels.image()?,
            Ok(None),
            request,
            cancel,
            false,
            if request.full {
                "Developed RAW"
            } else {
                "Half-size RAW development"
            },
        )?;
        // Half-size development must not change the reported sensor dimensions.
        if !request.full {
            decoded.original = original;
        }
        Ok(decoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn token() -> Cancellation {
        Cancellation {
            generation: Arc::new(AtomicU64::new(1)),
            id: 1,
        }
    }

    #[test]
    fn invalid_raw_and_cancelled_requests_do_not_publish_previews() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"not a camera file").unwrap();
        let request = DecodeRequest {
            path: file.path(),
            format: "DNG",
            orientation: 1,
            full: true,
            edge: 1200,
        };
        let mut published = false;
        assert!(decode_progressive(&request, &token(), Some(&mut |_| published = true)).is_err());
        assert!(!published);
        let cancel = token();
        cancel.generation.store(2, Ordering::Relaxed);
        assert_eq!(
            RawDecoder
                .decode_progressive(&request, &cancel, None)
                .err()
                .as_deref(),
            Some("Cancelled")
        );
    }

    #[test]
    #[ignore = "set OMP_NUM_THREADS=1; optionally override PHOTO_RAW_FILE"]
    fn profile_raw() {
        // X-Trans development in the linked LibRaw OpenMP build can vary between
        // runs. Compare pixels with one thread, without changing production settings.
        assert_eq!(std::env::var("OMP_NUM_THREADS").as_deref(), Ok("1"));
        let path = std::env::var_os("PHOTO_RAW_FILE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| crate::test_image("raw/Nikon_D2H.nef"));
        let request = DecodeRequest {
            path: &path,
            format: "RAW",
            orientation: 1,
            full: true,
            edge: 1200,
        };
        let cancel = token();
        let start = Instant::now();
        let mut preview_time = None;
        let progressive = decode_progressive(
            &request,
            &cancel,
            Some(&mut |preview| {
                assert!(preview.preview);
                assert!(preview.pixels.width().max(preview.pixels.height()) <= 1200);
                preview_time = Some(start.elapsed());
            }),
        )
        .unwrap();
        eprintln!(
            "RAW preview {:?}; full {:?}",
            preview_time.unwrap(),
            start.elapsed()
        );
        assert!(!progressive.preview);
        let direct = decode(&request, &cancel).unwrap();
        assert_eq!(progressive.pixels.dimensions(), direct.pixels.dimensions());
        assert!(
            progressive.pixels == direct.pixels,
            "preview extraction must not alter full development"
        );
        drop(direct);
        drop(progressive);

        // Cancellation triggered by the preview consumer must prevent full development.
        let cancel = token();
        let result = decode_progressive(
            &request,
            &cancel,
            Some(&mut |_| {
                cancel.generation.store(2, Ordering::Relaxed);
            }),
        );
        assert_eq!(result.err().as_deref(), Some("Cancelled"));

        // Exercise the same half-size development used when an embedded preview is absent.
        let cancel = token();
        let start = Instant::now();
        let raw = Raw::open(&path, true, &cancel).unwrap();
        let opened = start.elapsed();
        let width = unsafe { photo_raw_width(raw.ptr) };
        let height = unsafe { photo_raw_height(raw.ptr) };
        let mut pixels = Pixels::default();
        check(unsafe { photo_raw_develop(raw.ptr, &mut pixels) }).unwrap();
        eprintln!(
            "RAW open {opened:?}; half-size development {:?}; {} x {}",
            start.elapsed() - opened,
            pixels.width,
            pixels.height
        );
        assert!(
            u64::from(pixels.width) * u64::from(pixels.height)
                <= u64::from(width.div_ceil(2)) * u64::from(height.div_ceil(2))
        );
        assert!(pixels.image().is_ok());

        // Native path conversion must support non-ASCII filenames.
        let folder = tempfile::tempdir().unwrap();
        let unicode = folder.path().join("撮影-é.raw");
        std::fs::copy(&path, &unicode).unwrap();
        let request = DecodeRequest {
            path: &unicode,
            full: false,
            ..request
        };
        assert!(decode(&request, &cancel).unwrap().preview);
    }
}
