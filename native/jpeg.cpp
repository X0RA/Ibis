#include <turbojpeg.h>
#include <cstddef>
#include <cstdint>

// Use the stable TurboJPEG API, available on Linux, macOS and Windows.
extern "C" {
void *photo_jpeg_open(const unsigned char *bytes, size_t length, unsigned *width, unsigned *height) {
    tjhandle decoder = tjInitDecompress();
    if (!decoder) return nullptr;
    int w = 0, h = 0, sampling = 0, colorspace = 0;
    if (tjDecompressHeader3(decoder, bytes, static_cast<unsigned long>(length),
                            &w, &h, &sampling, &colorspace) != 0 || w <= 0 || h <= 0 ||
        colorspace == TJCS_CMYK || colorspace == TJCS_YCCK) {
        tjDestroy(decoder);
        return nullptr; // Retain the Rust decoder fallback for unusual JPEGs.
    }
    *width = static_cast<unsigned>(w);
    *height = static_cast<unsigned>(h);
    return decoder;
}
int photo_jpeg_read(void *decoder, const unsigned char *bytes, size_t length,
                    unsigned char *pixels, size_t capacity, unsigned width, unsigned height) {
    if (!decoder || !pixels || !width || !height ||
        uint64_t(width) * height * 3 != capacity) return -1;
    // Rust supplies exact 1, 1/2, 1/4 or 1/8 scaled dimensions. Accurate DCT
    // and normal chroma upsampling are retained, including in full-quality mode.
    return tjDecompress2(decoder, bytes, static_cast<unsigned long>(length), pixels,
                         static_cast<int>(width), 0, static_cast<int>(height), TJPF_RGB, 0);
}
void photo_jpeg_close(void *decoder) { tjDestroy(decoder); }
}
