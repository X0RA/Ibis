ibis for Windows x64 (experimental)

Run ibis.exe, optionally passing a photo, video or folder path.
Keep the supplied DLLs next to the executable. The installer installs these too.

External tools must be installed separately and available on PATH:
- ExifTool (https://exiftool.org): scanning and metadata writes.
  Rename exiftool(-k).exe to exiftool.exe and keep its exiftool_files folder.
- FFmpeg (https://ffmpeg.org): video thumbnails.
- Microsoft Visual C++ 2015-2022 x64 Redistributable if not already installed.

Settings and per-file records: %APPDATA%\ibis
Uninstalling keeps those records and does not change your media files.
The licenses folder records bundled library notices; consult the mpv build URL
for its source/dependency information. Windows packaging is best-effort and
has not yet been validated on a Windows development machine.
