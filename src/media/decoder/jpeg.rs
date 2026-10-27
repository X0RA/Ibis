use super::*;
use std::ffi::c_void;

unsafe extern "C" {
    fn photo_jpeg_open(
        bytes: *const u8,
        length: usize,
        width: *mut u32,
        height: *mut u32,
    ) -> *mut c_void;
    fn photo_jpeg_read(
        decoder: *mut c_void,
        bytes: *const u8,
        length: usize,
        pixels: *mut u8,
        capacity: usize,
        width: u32,
        height: u32,
    ) -> i32;
    fn photo_jpeg_close(decoder: *mut c_void);
}
struct Jpeg(*mut c_void);
impl Drop for Jpeg {
    fn drop(&mut self) {
        unsafe { photo_jpeg_close(self.0) };
    }
}

/// None means the native codec cannot handle this JPEG; use the Rust fallback.
pub(super) fn decode(
    bytes: &[u8],
    request: &DecodeRequest<'_>,
    cancel: &Cancellation,
) -> Result<Option<Decoded>> {
    let (mut width, mut height) = (0, 0);
    // All pointers refer to live buffers, and the opaque handle stays on this thread.
    let pointer = unsafe { photo_jpeg_open(bytes.as_ptr(), bytes.len(), &mut width, &mut height) };
    if pointer.is_null() {
        return Ok(None);
    }
    let decoder = Jpeg(pointer);
    check_size(width, height)?;
    let (icc, orientation) = {
        let mut metadata =
            image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes)).map_err(err)?;
        let own = metadata.orientation().map_err(err)?;
        let orientation = if own == Orientation::NoTransforms {
            Orientation::from_exif(request.orientation).unwrap_or(own)
        } else {
            own
        };
        (metadata.icc_profile().map_err(err)?, orientation)
    };
    let divisor = if request.full {
        1
    } else {
        [8, 4, 2, 1]
            .into_iter()
            .find(|&d| width.max(height).div_ceil(d) >= request.edge)
            .unwrap_or(1)
    };
    let (w, h) = (width.div_ceil(divisor), height.div_ceil(divisor));
    let mut pixels = vec![0; w as usize * h as usize * 3];
    cancel.check()?;
    // The output is exactly w*h*3 bytes and all buffers remain alive for this call.
    let code = unsafe {
        photo_jpeg_read(
            decoder.0,
            bytes.as_ptr(),
            bytes.len(),
            pixels.as_mut_ptr(),
            pixels.len(),
            w,
            h,
        )
    };
    if code != 0 {
        return Ok(None);
    }
    cancel.check()?;
    let image = image::RgbImage::from_raw(w, h, pixels).ok_or("Invalid JPEG pixels")?;
    let (target_width, target_height) = if request.full {
        (width, height)
    } else {
        fit_dimensions(width, height, request.edge)
    };
    // Use original aspect ratio, not ceil-rounded scaled JPEG dimensions.
    let mut image = resize_image(DynamicImage::ImageRgb8(image), target_width, target_height)?;
    image.apply_orientation(orientation);
    let profile = icc
        .as_deref()
        .map(ColorProfile::new_from_slice)
        .transpose()
        .map_err(err);
    let mut result = finish(image, profile, request, cancel, false, "")?;
    result.original = if matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    ) {
        (height, width)
    } else {
        (width, height)
    };
    Ok(Some(result))
}
