# ibis builds and dependencies

## Normal local builds

`make dev` builds and runs `target/debug/ibis`. `make build` builds
without launching; `make run ARGS="test_images"` passes an input path. These use
Cargo's normal `dev` profile, which already optimizes photo decoders while keeping
debug symbols. No custom profile, linker override or host-specific CPU flags are
added. `make release` explicitly builds a release binary. For Slint MCP inspection,
use `SLINT_EMIT_DEBUG_INFO=1 SLINT_MCP_PORT=9315 make dev CARGO_FLAGS="--features slint/mcp"`.

`make install PREFIX="$HOME/.local"` installs the debug binary, desktop file and
icon. Use `PROFILE=release` to install a release build. `DESTDIR` supports staging;
`make uninstall` with the same prefix removes only the installed app/launcher/icon,
leaving settings, ratings and cached thumbnails intact. Keep the prefix's `bin`
on PATH so the desktop launcher can find the executable.

## Linux system dependencies

Linux releases do **not** bundle libraries or external tools. They are built on
Ubuntu 24.04 x86_64; that is the compatibility baseline, not a universal static
binary. Other distros may need a local build against their own library versions.

Required at runtime:

- LibRaw 0.22+ (release builds currently link LibRaw 0.22.2).
- libheif 1.17+, with HEVC and AV1 decoder support (libde265 and dav1d/libaom).
- libjpeg-turbo and libmpv, plus their distro-managed dependencies.
- ExifTool on PATH for scanning and optional metadata writes.
- FFmpeg on PATH for video thumbnails.
- A graphical desktop with working OpenGL, fonts, and an XDG desktop portal
  backend appropriate to your desktop for folder selection.

Arch Linux:

```sh
sudo pacman -S libraw libheif libjpeg-turbo libde265 dav1d mpv ffmpeg perl-image-exiftool
# For source builds as well:
sudo pacman -S base-devel pkgconf
```

Ubuntu 24.04 runtime packages (enable Universe if necessary):

```sh
sudo apt install libmpv2 libheif1 libturbojpeg libheif-plugin-libde265 \
  libheif-plugin-aomdec ffmpeg libimage-exiftool-perl
```

Ubuntu 24.04's LibRaw package is older than the required 0.22 API. Install a
compatible 0.22.2 library from source or use a distro/package that provides it.
The CI setup script `scripts/setup-linux-ci.sh` shows the exact build, verifies the
source archive checksum, and installs to `/usr/local`; it also installs development
packages with sudo, so it is intended for build hosts rather than an app installer.
LibRaw sources: https://www.libraw.org/download
Ubuntu codec packages: https://packages.ubuntu.com/noble/libheif-plugin-aomdec

Source builds additionally need Rust stable, a C++ compiler, pkg-config and the
native development headers. On Ubuntu, see the apt list in `setup-linux-ci.sh`.
Run `make check-deps` before building, or
`bash scripts/check-linux-deps.sh target/debug/ibis` to check a built
binary and external tools. The latter uses `ldd`, so pass only a trusted binary.
It cannot guarantee that optional HEIF plugins or graphics drivers work.

## Linux release installer

Once releases are published, run:

```sh
bash scripts/install-linux.sh X0RA/Ibis "$HOME/.local"
# Optional third argument selects a release, e.g. v0.1.0; default is latest.
```

The repository defaults to [X0RA/Ibis](https://github.com/X0RA/Ibis); pass another
`OWNER/REPOSITORY` or set `IBIS_REPO` to install from a fork. The installer downloads
the release binary and verifies its SHA-256 checksum. Runtime dependency checks
and installation guidance are included in the installer. It fetches the launcher
and icon from the matching release tag in the repository, then installs them
alongside the binary. These resources and dependency notes are not separate
release assets. It does not install packages or change default file associations.

## macOS (best effort)

Build dependencies: `brew install mpv libraw libheif jpeg-turbo pkg-config exiftool ffmpeg`.
CI additionally uses `dylibbundler`. LibRaw must be 0.22 or newer. Apple Silicon
builds produce an unsigned/ad-hoc-signed DMG containing ibis.app and
linked dylibs; ExifTool and FFmpeg remain external Homebrew dependencies. HEIF
plugin relocation may need further work. The installer takes an optional `OWNER/REPOSITORY` (default `X0RA/Ibis`),
installs to `/Applications`, and does not remove macOS quarantine protection.
These packages are not notarized; use macOS Privacy & Security to approve them.

## Windows (best effort)

Use Rust stable MSVC, Visual Studio C++ Build Tools, 7-Zip and a current vcpkg
checkout. Set `VCPKG_ROOT`, then run `scripts/powershell/setup-deps.ps1` in the
same PowerShell session used for `cargo build`. It installs LibRaw, libjpeg-turbo,
libheif with libde265/dav1d, and obtains mpv's DLL and MSVC import library.
LibRaw's port must be 0.22+. CI stages the native DLLs and library notices into a
ZIP and Inno Setup installer. ExifTool, FFmpeg and the Visual C++ runtime are
external requirements; see `scripts/installer/windows-readme.txt`.

Windows/macOS jobs remain enabled, but are not validated from this Linux host.
Before distributing those bundled native libraries, check their exact licenses,
source/relinking requirements, HEIF plugin paths and platform runtime behavior.
No application LICENSE was present in this checkout; packaging does not invent one.

## Workflows

CI runs formatting, Clippy, debug tests, shell/desktop checks, and a staged Linux
install/uninstall. Releases retain the copied workflow's automatic patch-tagging
on main pushes, explicit version/tag support, Linux/Windows/macOS builds, and
SHA256SUMS. The resolved release number is written into Cargo.toml/Cargo.lock
before each platform build and embedded in the binary. Settings and `ibis --version`
share this value; each release job checks the executable reports the expected version.
Local builds use the version in Cargo.toml. All platform builds must succeed before publishing. No release is
published by running local make targets.
