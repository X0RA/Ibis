#!/usr/bin/env bash
# Check system dependencies without installing packages or bundling libraries.
set -euo pipefail
if [[ $(uname -s) != Linux ]]; then
    echo 'This dependency checker is for Linux.' >&2
    exit 1
fi
if [[ ${1:-} == --help ]]; then
    echo 'Usage: check-linux-deps.sh [--build | /path/to/ibis]'
    exit 0
fi
missing=0
for tool in exiftool ffmpeg; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Missing runtime tool: $tool" >&2
        missing=1
    fi
done
if [[ ${1:-} == --build ]]; then
    for tool in cargo c++ pkg-config; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            echo "Missing build tool: $tool" >&2
            missing=1
        fi
    done
    if command -v pkg-config >/dev/null 2>&1; then
        for dependency in 'libraw >= 0.22' 'libheif >= 1.17' 'libturbojpeg' 'mpv >= 2.2'; do
            if ! pkg-config --exists "$dependency"; then
                echo "Missing development library: $dependency" >&2
                missing=1
            fi
        done
    fi
elif [[ -n ${1:-} ]]; then
    if [[ ! -f $1 ]]; then echo "Binary not found: $1" >&2; exit 1; fi
    if ! links=$(ldd "$1" 2>&1); then
        echo "$links" >&2
        missing=1
    elif [[ $links == *'not found'* ]]; then
        echo "$links" >&2
        missing=1
    fi
else
    echo 'Usage: check-linux-deps.sh [--build | /path/to/ibis]' >&2
    exit 2
fi
if (( missing )); then
    cat >&2 <<'HELP'
Install system dependencies before running ibis (see packaging/README.md).
Arch: sudo pacman -S libraw libheif libjpeg-turbo libde265 dav1d mpv ffmpeg perl-image-exiftool
Ubuntu 24.04: sudo apt install libmpv2 libheif1 libturbojpeg libheif-plugin-libde265 libheif-plugin-aomdec ffmpeg libimage-exiftool-perl
LibRaw 0.22+ is required; Ubuntu 24.04's older LibRaw package is insufficient.
Release binaries target Ubuntu 24.04 x86_64 or compatible systems. Build locally
if your distro uses incompatible library SONAMEs. An OpenGL-capable desktop and
an XDG desktop portal implementation are also needed for the GUI/folder picker.
HELP
    exit 1
fi
echo 'Required tools and linked/build libraries are available.'
echo 'HEIC/AVIF also require libheif decoder plugins (libde265 and aom/dav1d).'
