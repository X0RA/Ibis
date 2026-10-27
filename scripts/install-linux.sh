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
for tool in curl sha256sum install; do command -v "$tool" >/dev/null || { echo "Missing $tool" >&2; exit 1; }; done
WORK_DIR=$(mktemp -d -t ibis-install.XXXXXX)
trap 'rm -rf "$WORK_DIR"' EXIT
BASE="https://github.com/$REPO/releases/latest/download"
if [[ $TAG != latest ]]; then BASE="https://github.com/$REPO/releases/download/$TAG"; fi
FILES=(ibis-linux-x86_64 ibis.desktop ibis.png check-linux-deps.sh linux-dependencies.md)
curl -fLsS "$BASE/SHA256SUMS" -o "$WORK_DIR/SHA256SUMS"
for file in "${FILES[@]}"; do
    curl -fL --progress-bar "$BASE/$file" -o "$WORK_DIR/$file"
    checksum=$(awk -v name="$file" '$2 == name {print $1}' "$WORK_DIR/SHA256SUMS")
    [[ $checksum =~ ^[a-fA-F0-9]{64}$ ]] || { echo "Missing/invalid checksum for $file" >&2; exit 1; }
    (cd "$WORK_DIR" && printf '%s  %s\n' "$checksum" "$file" | sha256sum -c -)
done
chmod +x "$WORK_DIR/ibis-linux-x86_64"
if ! bash "$WORK_DIR/check-linux-deps.sh" "$WORK_DIR/ibis-linux-x86_64"; then
    cat "$WORK_DIR/linux-dependencies.md" >&2
    exit 1
fi
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
"${SUDO[@]}" install -Dm644 "$WORK_DIR/linux-dependencies.md" "$PREFIX/share/doc/ibis/linux-dependencies.md"
if command -v update-desktop-database >/dev/null; then "${SUDO[@]}" update-desktop-database "$PREFIX/share/applications"; fi
if command -v gtk-update-icon-cache >/dev/null; then "${SUDO[@]}" gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor"; fi
echo "Installed ibis to $PREFIX. Ensure $PREFIX/bin is on PATH."
