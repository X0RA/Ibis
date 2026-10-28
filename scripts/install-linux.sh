#!/usr/bin/env bash
set -euo pipefail
# Defaults to the upstream repository; pass OWNER/REPOSITORY or set IBIS_REPO for a fork.
REPO="${IBIS_REPO:-X0RA/Ibis}"
PREFIX="${PREFIX:-/usr/local}"
TAG="${IBIS_TAG:-latest}"
usage() { echo 'Usage: install-linux.sh [OWNER/REPOSITORY] [PREFIX] [vX.Y.Z|latest]  (default repository: X0RA/Ibis)'; }
if [[ ${1:-} == --help ]]; then usage; exit 0; fi
REPO="${1:-$REPO}"
PREFIX="${2:-$PREFIX}"
TAG="${3:-$TAG}"
if [[ ! $REPO =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || [[ $PREFIX != /* ]]; then
    usage >&2; exit 2
fi
if [[ $TAG != latest && ! $TAG =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then usage >&2; exit 2; fi
if [[ $(uname -s) != Linux || $(uname -m) != x86_64 ]]; then
    echo 'Only Linux x86_64 release binaries are published.' >&2; exit 1
fi
for tool in curl sha256sum install awk ldd; do command -v "$tool" >/dev/null || { echo "Missing $tool" >&2; exit 1; }; done
WORK_DIR=$(mktemp -d -t ibis-install.XXXXXX)
trap 'rm -rf "$WORK_DIR"' EXIT
# Resolve latest once so the binary and desktop resources use the same release.
if [[ $TAG == latest ]]; then
    release_url=$(curl -fLsS -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest")
    TAG=${release_url##*/}
    [[ $TAG =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo 'Could not resolve latest release tag' >&2; exit 1; }
fi
BASE="https://github.com/$REPO/releases/download/$TAG"
FILES=(ibis-linux-x86_64)
curl -fLsS "$BASE/SHA256SUMS" -o "$WORK_DIR/SHA256SUMS"
for file in "${FILES[@]}"; do
    curl -fL --progress-bar "$BASE/$file" -o "$WORK_DIR/$file"
    checksum=$(awk -v name="$file" '$2 == name {print $1}' "$WORK_DIR/SHA256SUMS")
    [[ $checksum =~ ^[a-fA-F0-9]{64}$ ]] || { echo "Missing/invalid checksum for $file" >&2; exit 1; }
    (cd "$WORK_DIR" && printf '%s  %s\n' "$checksum" "$file" | sha256sum -c -)
done
chmod +x "$WORK_DIR/ibis-linux-x86_64"
missing=0
for tool in exiftool ffmpeg; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Missing runtime tool: $tool" >&2
        missing=1
    fi
done
if ! links=$(ldd "$WORK_DIR/ibis-linux-x86_64" 2>&1); then
    echo "$links" >&2
    missing=1
elif [[ $links == *'not found'* ]]; then
    echo "$links" >&2
    missing=1
fi
if (( missing )); then
    cat >&2 <<'HELP'
Install system dependencies before running ibis.
Arch: sudo pacman -S libraw libheif libjpeg-turbo libde265 dav1d mpv ffmpeg perl-image-exiftool
Ubuntu 24.04: sudo apt install libmpv2 libheif1 libturbojpeg libheif-plugin-libde265 libheif-plugin-aomdec ffmpeg libimage-exiftool-perl
LibRaw 0.22+ is required; Ubuntu 24.04's older LibRaw package is insufficient.
Release binaries target Ubuntu 24.04 x86_64 or compatible systems. Build locally
if your distro uses incompatible library SONAMEs. An OpenGL-capable desktop and
an XDG desktop portal implementation are also needed for the GUI/folder picker.
HELP
    echo "More details: https://github.com/$REPO/blob/$TAG/packaging/README.md" >&2
    exit 1
fi
echo 'HEIC/AVIF also require libheif decoder plugins (libde265 and aom/dav1d).'
# Desktop resources live in the repository, not as separate release assets.
SOURCE="https://raw.githubusercontent.com/$REPO/$TAG"
curl -fLsS "$SOURCE/packaging/ibis.desktop" -o "$WORK_DIR/ibis.desktop"
curl -fLsS "$SOURCE/ui/assets/appicon.png" -o "$WORK_DIR/ibis.png"
SUDO=()
# Resolve an existing ancestor for first-time user-prefix installations.
ancestor=$PREFIX
while [[ ! -d $ancestor ]]; do ancestor=$(dirname "$ancestor"); done
if [[ ! -w $ancestor || ( -d $PREFIX/bin && ! -w $PREFIX/bin ) ]]; then
    command -v sudo >/dev/null || { echo "Write permission or sudo required for $PREFIX" >&2; exit 1; }
    SUDO=(sudo)
fi
"${SUDO[@]}" install -Dm755 "$WORK_DIR/ibis-linux-x86_64" "$PREFIX/bin/ibis"
"${SUDO[@]}" install -Dm644 "$WORK_DIR/ibis.desktop" "$PREFIX/share/applications/ibis.desktop"
"${SUDO[@]}" install -Dm644 "$WORK_DIR/ibis.png" "$PREFIX/share/icons/hicolor/256x256/apps/ibis.png"
if command -v update-desktop-database >/dev/null; then "${SUDO[@]}" update-desktop-database "$PREFIX/share/applications"; fi
if command -v gtk-update-icon-cache >/dev/null; then "${SUDO[@]}" gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor"; fi
echo "Installed ibis to $PREFIX. Ensure $PREFIX/bin is on PATH."
