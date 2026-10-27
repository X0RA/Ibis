# Image format test set

All generated images are tiny (64×64, same gradient + circle + white square) so you can compare decoders on encoding alone. Camera RAW files are real camera captures, sourced from raw.pixls.us under CC0 / public domain.

## common/
| File | Notes |
|---|---|
| sample.jpg | Baseline JPEG |
| sample_progressive.jpg | Progressive JPEG |
| sample.png | PNG with alpha |
| sample.gif | GIF, single frame, 1-bit transparency |
| sample_animated.gif | GIF, 8 frames, looping |
| sample.bmp | Windows BMP |
| sample.tiff | TIFF, LZW, with alpha |
| sample.tif | TIFF, Deflate, `.tif` extension |

## web/
| File | Notes |
|---|---|
| sample.webp / sample_lossless.webp | WebP lossy + lossless, alpha |
| sample_animated.webp | Animated WebP, 8 frames |
| sample.avif / sample_animated.avif | AVIF still + animated sequence |
| sample_animated.apng | Animated PNG |
| sample.jxl / sample_lossless.jxl | JPEG XL lossy + lossless, alpha |
| sample.heic | HEIF/HEIC (HEVC) |
| sample.svg / sample.svgz | SVG vector + gzipped SVG |
| sample.qoi | QOI (Quite OK Image) |

## icon/
| File | Notes |
|---|---|
| favicon.ico | Multi-size ICO (16, 32, 48) |
| sample.cur | Windows cursor |
| sample.icns | macOS icon (16, 32, 128 PNG entries) |

## editor/
| File | Notes |
|---|---|
| sample.psd | Photoshop (flattened) |
| sample.tga | Targa |
| sample.sgi | SGI / RGB |
| sample.dds | DirectDraw Surface (game textures) |
| sample.miff | ImageMagick native |
| sample.pdf / sample.eps | Raster wrapped in PDF / Encapsulated PostScript |

## hdr-pro/
| File | Notes |
|---|---|
| sample.exr | OpenEXR, 32-bit float, ZIP, values up to 4.0 (real HDR range) |
| sample.hdr | Radiance RGBE |
| sample.pfm | Portable Float Map |
| sample.dpx | Digital Picture Exchange (film/VFX) |
| sample.jp2 / sample.j2k | JPEG 2000 (JP2 container + raw codestream) |
| sample.jxr | JPEG XR / HD Photo |
| sample_16bit.tiff | 16 bits per channel TIFF |

## legacy/
pcx, ppm, pgm, pbm, pam (Netpbm family), xbm, xpm, xwd (X11), ras (Sun Raster), wbmp, jng, mng, fits (astronomy), pict (classic Mac), jbig (bi-level).

## raw/ (real camera files)
| File | Format | Camera |
|---|---|---|
| Canon_EOS-40D.cr2 | CR2 (sRAW) | Canon EOS 40D |
| Canon_EOS-R6.cr3 | CR3 | Canon EOS R6 |
| Canon_PowerShot-G1.crw | CRW (CIFF) | Canon PowerShot G1 |
| Nikon_D2H.nef | NEF | Nikon D2H |
| Sony_ILCE-7S.arw | ARW | Sony A7S |
| Fujifilm_FinePix-S5000.raf | RAF | Fujifilm FinePix S5000 |
| Canon_EOS-5D-Mark-III.dng | DNG | Canon 5D III (Adobe-converted) |
| Olympus_E-10.orf | ORF | Olympus E-10 |
| Panasonic_DMC-LX7.rw2 | RW2 | Panasonic LX7 |
| Pentax_K10D.pef | PEF | Pentax K10D |
| Sigma_DP1.x3f | X3F (Foveon) | Sigma DP1 |
| Minolta_DiMAGE-5.mrw | MRW | Minolta DiMAGE 5 |
| GoPro_HERO7-Black.gpr | GPR (DNG + VC-5) | GoPro HERO7 Black |

Edge cases worth knowing: GPR needs the GoPro GPR SDK, so stock LibRaw/ImageMagick reject it. X3F is dropped by many newer raw libraries. The CR2 is an sRAW variant, which some decoders handle differently from full-size CR2.

## video/

`sample.mp4` is a one-second 64×48 H.264 test pattern used to verify video
detection even when renamed with an image extension. Generated with FFmpeg:

```sh
ffmpeg -f lavfi -i 'testsrc2=size=64x48:rate=6:duration=1' -c:v libx264 -pix_fmt yuv420p -movflags +faststart sample.mp4
```

## Automated tests

Tests use selected supported files from this directory. This is a format exploration
set, not a claim that every file currently decodes. AVIF/JPEG XL colour and alpha
checks compare with `common/sample.png`. The opt-in RAW smoke test and benchmark
default to `raw/Nikon_D2H.nef`; the JPEG benchmark defaults to `common/sample.jpg`.
Renaming, metadata writes, timestamp changes and derived size/orientation cases
use temporary copies. Tests never modify these source files.
