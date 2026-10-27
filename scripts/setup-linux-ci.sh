#!/usr/bin/env bash
# Ubuntu 24.04 CI host setup. Not run by the user-facing release installer.
set -euo pipefail
sudo apt-get update
sudo apt-get install -y build-essential clang cmake pkg-config curl ca-certificates \
    desktop-file-utils shellcheck libfontconfig-dev libgl1-mesa-dev libmpv-dev \
    libwayland-dev libx11-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxcursor-dev libxi-dev libxkbcommon-dev libxrandr-dev \
    libheif-dev libheif-plugin-libde265 libheif-plugin-aomdec \
    libturbojpeg0-dev libjpeg-dev liblcms2-dev zlib1g-dev \
    ffmpeg libimage-exiftool-perl
if ! pkg-config --atleast-version=0.22 libraw; then
    work=$(mktemp -d -t ibis-libraw.XXXXXX)
    trap 'rm -rf "$work"' EXIT
    curl -fLsS --retry 3 https://www.libraw.org/data/LibRaw-0.22.2.tar.gz -o "$work/libraw.tar.gz"
    (cd "$work" && printf '%s  libraw.tar.gz\n' de86b035655accff8d4010f1a221fdf50d353cb7b1422ba26f14a0db92612cfa | sha256sum -c -)
    tar -xzf "$work/libraw.tar.gz" -C "$work"
    (
        cd "$work/LibRaw-0.22.2"
        ./configure --prefix=/usr/local --disable-static --disable-examples
        make -j"$(nproc)"
        sudo make install
    )
    sudo ldconfig
fi
bash "$(dirname "$0")/check-linux-deps.sh" --build
