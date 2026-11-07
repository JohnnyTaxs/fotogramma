# Native build and distribution

The directory is named `deployment/` rather than `packaging/` to avoid a conflict with the Python `packaging` package required by PyInstaller.

## Requirements

For building:

- Python 3.13 recommended;
- `uv`;
- PySide6 Essentials and PyInstaller downloaded in a temporary environment;
- on macOS, `ffprobe` installed through Homebrew and Xcode tools for `otool`;
- on Windows and Linux, network access to download static FFmpeg builds.

None of these are required on the destination computer.

## Build

From the project root, on the target operating system:

```bash
uv run --no-project --with pyside6-essentials --with pyinstaller==6.22.3 \
  python deployment/build.py
```

The command prepares `ffprobe`, runs PyInstaller, and produces:

- `Fotogramma.app` on macOS;
- `dist/Fotogramma/` on Windows and Linux;
- `dist/Fotogramma-<OS>-<arch>.zip` for transport.

`build/`, `dist/`, and `deployment/vendor/` are ignored by Git.

## Contents

The **onedir** build includes Python, PySide6 Essentials, Qt plugins, fonts, licenses, `ffmpeg`, and `ffprobe`. It does not include a browser or Qt WebEngine.

`runtime.py` uses `sys._MEIPASS` to locate assets in the relocated package. In frozen mode, `ffprobe` resolution always prefers the binary included in the bundle.

## ffmpeg and ffprobe by platform

### macOS

`deployment/prepare_ffprobe.py`:

1. copies `ffmpeg` and `ffprobe` from the Homebrew installation used for the build;
2. reads dependencies with `otool -L`;
3. recursively copies non-system dylibs into `lib/`;
4. runs both binaries with a local `DYLD_LIBRARY_PATH`;
5. verifies with `DYLD_PRINT_LIBRARIES` that no library is loaded from Homebrew.

`deployment/vendor/<OS>-<arch>/manifest.json` records the SHA-256 and size of every collected file.

### Windows and Linux

The downloader extracts `ffmpeg` and `ffprobe` (or the corresponding `.exe` files) from the static gyan.dev and johnvansickle.com archives. It does not extract complete directories and never uses an archive path to write outside the vendor folder.

## Verification

```bash
# Start the GUI
Fotogramma --directory /path/to/fixtures --scan-on-start --smoke-test

# Explicit macOS path
Fotogramma.app/Contents/MacOS/Fotogramma \
  --directory /path/to/fixtures --scan-on-start --smoke-test
```

Smoke-test mode closes automatically after startup or after the scan completes.

Before release:

1. Extract the ZIP in a new directory.
2. Start with `PATH=/usr/bin:/bin`.
3. Verify startup, fonts, and a complete scan.
4. Extract the ZIP again elsewhere and repeat.
5. Verify that no external native libraries are loaded.
6. Convert a short H.264 with the bundled `ffmpeg` and confirm `hevc` with the bundled `ffprobe`.

For verified replacement, use only temporary copies of fixtures. Confirm that codec, duration, streams, and full decode are validated before renaming, and that the `.fotogramma-backup` is deleted only after success. Simulate a failed test to confirm that the original is preserved.

The `Fotogramma-Darwin-arm64.zip` build has been verified with this workflow. Windows and Linux require the same verification on their own systems.

## Signing

The macOS build is ad-hoc signed but not notarized. Use right-click → Open on first launch. Public distribution without warnings requires a Developer ID certificate and Apple notarization. Windows SmartScreen may also require a code-signing strategy.
