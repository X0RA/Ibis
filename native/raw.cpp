#include <libraw/libraw.h>
#include <new>
#include <cstddef>

#if LIBRAW_VERSION < LIBRAW_MAKE_VERSION(0, 22, 0)
#error "Photo viewer requires LibRaw 0.22 or newer"
#endif

// Keep LibRaw's version-dependent structs on the C++ side of the boundary.
// Every object is confined to one decoder worker; LibRaw owns its file stream.
extern "C" {
struct PhotoPixels {
    const unsigned char *data;
    size_t size;
    unsigned width, height, colors, bits, jpeg;
    void *owner;
};
typedef int (*PhotoCancel)(void *);
struct PhotoRaw {
    LibRaw raw;
    PhotoCancel cancelled;
    void *context;
};
static int progress(void *p, enum LibRaw_progress, int, int) {
    auto *r = static_cast<PhotoRaw *>(p);
    return r->cancelled && r->cancelled(r->context);
}
void *photo_raw_open(const void *path, int half_size, PhotoCancel cb,
                     void *context, int *error) {
    auto *r = new (std::nothrow) PhotoRaw;
    if (!r) { *error = LIBRAW_UNSUFFICIENT_MEMORY; return nullptr; }
    r->cancelled = cb;
    r->context = context;
    r->raw.set_progress_handler(progress, r);
    r->raw.imgdata.rawparams.max_raw_memory_mb = 768;
    r->raw.imgdata.params.use_camera_wb = 1;
    r->raw.imgdata.params.half_size = half_size;
#ifdef _WIN32
    const auto *filename = static_cast<const wchar_t *>(path);
#else
    const auto *filename = static_cast<const char *>(path);
#endif
#ifndef LIBRAW_NO_IOSTREAMS_DATASTREAM
    *error = r->raw.open_file(filename, 0); // Seekable stdio; no whole-file buffer.
#else
    *error = r->raw.open_file(filename);
#endif
    if (*error) { delete r; return nullptr; }
    return r;
}
void photo_raw_close(void *p) { delete static_cast<PhotoRaw *>(p); }
const char *photo_raw_error(int code) { return libraw_strerror(code); }
unsigned photo_raw_width(void *p) { return static_cast<PhotoRaw *>(p)->raw.imgdata.sizes.width; }
unsigned photo_raw_height(void *p) { return static_cast<PhotoRaw *>(p)->raw.imgdata.sizes.height; }
static int pixels(libraw_processed_image_t *im, int error, PhotoPixels *out) {
    if (!im) return error ? error : LIBRAW_UNSPECIFIED_ERROR;
    *out = {im->data, im->data_size, im->width, im->height, im->colors,
            im->bits, im->type == LIBRAW_IMAGE_JPEG ? 1u : 0u, im};
    return 0;
}
int photo_raw_preview(void *p, PhotoPixels *out) {
    auto &r = static_cast<PhotoRaw *>(p)->raw;
    int err = r.unpack_thumb();
    if (err) return err;
    auto *im = r.dcraw_make_mem_thumb(&err);
    return pixels(im, err, out);
}
int photo_raw_develop(void *p, PhotoPixels *out) {
    auto &r = static_cast<PhotoRaw *>(p)->raw;
    r.imgdata.params.use_camera_wb = 1;
    r.imgdata.params.output_color = 1; // sRGB primaries
    r.imgdata.params.output_bps = 8;
    r.imgdata.params.gamm[0] = 1.0 / 2.4;
    r.imgdata.params.gamm[1] = 12.92;
    // LibRaw applies camera orientation to developed pixels.
    int err = r.unpack();
    if (err) return err;
    err = r.dcraw_process();
    if (err) return err;
    auto *im = r.dcraw_make_mem_image(&err);
    return pixels(im, err, out);
}
void photo_raw_free(void *owner) {
    LibRaw::dcraw_clear_mem(static_cast<libraw_processed_image_t *>(owner));
}
}
