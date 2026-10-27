<p align="center">
  <img src="ui/assets/appicon.png" alt="ibis icon" width="128">
</p>

# ibis

A fast desktop photo and video viewer written in Rust with [Slint](https://slint.dev). Point it at a folder and it shows your photos and videos in a grid sorted by capture date, with RAW support, ratings, filters and built-in video playback.

ibis works directly with your folders, making it useful for browsing a collection or culling a shoot. RAW previews keep navigation responsive, with full-resolution development available when needed.

## Features

- **Library:** grid or list view, grouped by day, month or year with a scrollable timeline. Sort by name, type, date, source or rating.
- **Wide format support:** JPEG, PNG, WebP, TIFF, HEIC/AVIF, JPEG XL and camera RAW files (CR2, CR3, NEF, ARW, RAF, DNG and more).
- **Photo viewer:** zoom, pan, fit or 100%, rotate, and a full quality mode that develops RAWs at full resolution. Nearby photos are preloaded so moving between them is fast.
- **Video playback:** plays videos in the same window using mpv, with seeking, volume, frame stepping and hardware decoding.
- **Culling:** star ratings, reject (X), trash with undo, and move all rejected files to another folder in one go.
- **RAW + JPEG pairing:** shows matching RAW and JPEG files as one item, and **B** switches between them.
- **Filters:** by media type, rating, camera or date range.
- **Inspector:** camera, lens, exposure, dimensions, GPS and other metadata.
- **Slideshow, filmstrip and fullscreen**, plus keyboard shortcuts for almost everything.
- **Live folder watching:** new, removed and renamed files show up automatically.
- **Optional metadata writing:** save ratings straight into the file's XMP instead of keeping them in the app.
- **Clipboard and file manager:** copy the image or its path, or show the file in your file manager.

## Installation

Get the latest build from [GitHub Releases](https://github.com/X0RA/Ibis/releases), or use the Linux/macOS installers below once releases are published. Prebuilt apps do not need Rust or a compiler. Windows and macOS packaging is still experimental.

### Linux

The x86_64 build uses system libraries: LibRaw 0.22+, libjpeg-turbo, libheif 1.17+ with HEIC/AVIF decoders, and libmpv. [ExifTool](https://exiftool.org/) handles scanning and metadata; FFmpeg generates video thumbnails. A working OpenGL desktop and an XDG desktop portal backend are also needed.

**Arch runtime packages:**

```sh
sudo pacman -S libraw libheif libjpeg-turbo libde265 dav1d mpv ffmpeg perl-image-exiftool
```

**Ubuntu 24.04 runtime packages:**

```sh
sudo apt install libmpv2 libheif1 libturbojpeg libheif-plugin-libde265 \
  libheif-plugin-aomdec ffmpeg libimage-exiftool-perl
```

Ubuntu 24.04 also needs a newer LibRaw: the release links to 0.22.2, which is not supplied by its stock package. See [Development](#development) for the source-build instructions. Release binaries target Ubuntu 24.04; other distros may need a local build against their library versions.

Then install ibis:

```bash
curl -fsSL https://raw.githubusercontent.com/X0RA/Ibis/main/scripts/install-linux.sh | PREFIX="$HOME/.local" bash
```

Ensure `~/.local/bin` is on your PATH. The installer checks dependencies but does not install them.

### macOS

The Apple Silicon app bundles its linked native libraries. Install the external runtime tools with Homebrew:

```sh
brew install exiftool ffmpeg
```

Then install ibis into `/Applications`:

```bash
curl -fsSL https://raw.githubusercontent.com/X0RA/Ibis/main/scripts/install-macos.sh | bash
```

The app is not notarized; macOS may require approval in **System Settings → Privacy & Security**.

### Windows

Download the x64 installer or ZIP from [GitHub Releases](https://github.com/X0RA/Ibis/releases). For the ZIP, keep the supplied DLLs alongside `ibis.exe`.

Install [ExifTool](https://exiftool.org/) and [FFmpeg](https://ffmpeg.org/) separately and add them to PATH. Rename `exiftool(-k).exe` to `exiftool.exe`, keeping its `exiftool_files` folder alongside it. The Microsoft Visual C++ 2015–2022 x64 Redistributable is also required if it is not already installed.

## Usage

```sh
ibis                       # opens a folder picker
ibis ~/Pictures            # open a folder (scans subfolders too)
ibis ~/Pictures/photo.NEF  # open a single file, browsing the rest of its folder
ibis -r ~/Pictures --list  # print the files in date order without opening the GUI
ibis --cache-mib 1024      # use a larger photo memory cache (default 256 MiB)
```

Run `ibis --help` for all options.

In the viewer, **← / →** changes items, **I** toggles the inspector, **R** rotates,
**X** toggles rejection, and **F / F11** toggles fullscreen. For video, **Space**
plays/pauses and **J / L** seeks backward/forward ten seconds.

Ratings and rejection stay in local JSON records by default. Enable file metadata
writing in **Settings** to save them as embedded XMP instead; switching modes does
not migrate existing ratings.

## Development

Source builds need stable Rust, a C++ compiler, LibRaw 0.22+, libjpeg-turbo, libheif 1.17+ with HEIC/AVIF decoders, and libmpv development files, plus the runtime tools listed above.

### Linux build dependencies

On **Arch**, install the runtime packages above, then:

```sh
sudo pacman -S base-devel pkgconf
```

On **Ubuntu 24.04**, install the runtime packages above, then:

```sh
sudo apt install build-essential clang cmake pkg-config curl ca-certificates \
  libfontconfig-dev libgl1-mesa-dev libwayland-dev libx11-dev libxcb-shape0-dev \
  libxcb-xfixes0-dev libxcursor-dev libxi-dev libxkbcommon-dev libxrandr-dev \
  libmpv-dev libheif-dev libturbojpeg0-dev libjpeg-dev liblcms2-dev zlib1g-dev
```

Build LibRaw 0.22.2 from [source](https://www.libraw.org/download). [scripts/setup-linux-ci.sh](scripts/setup-linux-ci.sh) contains the exact download, checksum and build commands; running the whole script also installs the Ubuntu development packages with sudo.

### macOS build dependencies

```sh
xcode-select --install
brew install mpv libraw libheif jpeg-turbo pkg-config exiftool ffmpeg
export PKG_CONFIG_PATH="$(brew --prefix jpeg-turbo)/lib/pkgconfig:$(brew --prefix)/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
```

### Windows build dependencies

Install Rust (MSVC), Visual Studio C++ Build Tools, 7-Zip and a current [vcpkg](https://vcpkg.io) checkout. Set `VCPKG_ROOT`, then run this from the cloned repository in the same PowerShell session used to build:

```powershell
scripts/powershell/setup-deps.ps1
cargo build --locked
```

The setup script installs the photo decoder libraries and prepares libmpv. ExifTool and FFmpeg remain separate runtime installs.

### Build and run

```sh
git clone https://github.com/X0RA/Ibis.git
cd Ibis
make dev       # build and run target/debug/ibis
make check
make fmt
cargo test
```

UI components have standalone previews with mock data in `ui/previews/`. See [packaging/README.md](packaging/README.md) for dependency checks, packaging and release details.
