# Fotogramma

Fotogramma is a **native desktop application** for inspecting a video library, identifying files that may benefit from H.265 conversion, searching for an existing H.265 version online, and converting videos directly with FFmpeg.

It is built with **PySide6/Qt Widgets**, not a browser, WebView, HTTP server, or launcher.

![Fotogramma Desktop](docs/assets/fotogramma-native-results.png)

> **Personal project:** Fotogramma was created primarily for my own use. It is shared in the hope that it may also be useful to others. **It is provided “AS IS”, without any warranty of any kind, express or implied, including but not limited to correctness, reliability, fitness for a particular purpose, non-infringement, availability, security, or data integrity. Use it at your own risk. You are solely responsible for backups and for validating converted or replaced files.**

## Features

- Native desktop window for macOS, Windows, and Linux.
- Recursive discovery of `.mp4`, `.mkv`, and `.webm` files.
- Clear grouping into **to convert**, **already compatible** (HEVC/AV1), and **unreadable** files.
- Statistics, filters, search, sortable columns, and multi-file selection.
- **Find H.265 online** opens a cleaned-up search on 1337x.to in your default browser.
- **Convert selected** runs FFmpeg directly with readable presets and a custom mode.
- Full-output validation before replacing an original file.
- Original application icon, Cinema Pop theme, and bundled fonts.
- No videos are modified during inspection. No media or metadata is uploaded during scanning or conversion.

## Conversion presets

| Preset | Intended use | Video settings | Default audio |
| --- | --- | --- | --- |
| Maximum quality and compression | Best quality/size ratio, even if slow | CRF 20, `veryslow`, AQ mode 3, 10-bit | Original |
| Intermediate · good quality | Balanced quality, size, and speed | CRF 22, `medium`, 10-bit | Original |
| Fast · quick transcode | Maximum transcoding speed | CRF 26, `ultrafast`, 8-bit | Original |
| Custom | Manual control | CRF, preset, pixel format, tune, AQ mode | Original or AAC |

All streams are mapped with `-map 0`, preserving audio tracks, subtitles, chapters, and compatible metadata. Audio can be kept unchanged or encoded as **AAC 128/160/256 kbps**. MKV is recommended for stream compatibility; MP4 enables `+faststart`.

The default output is a new `.h265.mkv` or `.h265.mp4` file with a numeric suffix if needed. **Originals are never overwritten in the default mode.**

## Verified original replacement

**Replace originals only after full validation** is an explicit, disabled-by-default option. When enabled, Fotogramma writes a `.partial` file next to the original and replaces it only after all checks pass:

1. Output codec is HEVC.
2. Duration matches within 1% or 1 second.
3. Audio, video, subtitle, and data stream counts match.
4. Full decode succeeds with `ffmpeg -v error -i output -f null -`.
5. The replacement passes a final codec check before the temporary backup is deleted.

If any check fails, the original remains unchanged. Replacement uses an atomic rename on the same filesystem and a `.fotogramma-backup` file that is removed only after success. The container must match the original extension (MKV or MP4); WebM replacement is not supported.

## Online search

Select one convertible video and choose **Find H.265 online**. Fotogramma opens `https://1337x.to/search/<title+H.265>/1/` in your default browser.

No content is downloaded, no torrent client is integrated, and no WebView is embedded. Common release tags are removed from the title to improve search results. A query is sent outside your computer only when you explicitly choose this action.

## Run the portable app

The portable build includes **Python, Qt, fonts, ffmpeg, and ffprobe**, so no installation is required on the destination computer.

For macOS Apple Silicon, download `Fotogramma-Darwin-arm64.zip` from the [latest release](https://github.com/JohnnyTaxs/fotogramma/releases/latest), then extract it with Finder (double-click). The bundle is ad-hoc signed but not notarized, so Gatekeeper asks for confirmation on first launch: use **System Settings → Privacy & Security → Open Anyway** (on older macOS versions, right-click `Fotogramma.app` → **Open**).

Do not extract the ZIP with `unzip` or tools that turn symbolic links into copies: Qt and Python frameworks rely on those links, and the app bundle would no longer start. The `ditto`-based archive keeps them intact, and `deployment/build.py` verifies the extracted bundle signature before publishing.

The portable build bundles `ffmpeg` and `ffprobe`, so no separate media tools are required.

On Windows, run `Fotogramma.exe` inside the extracted folder. On Linux, run the `Fotogramma` binary. Each package must be built on its own operating system: PyInstaller is not a cross-compiler.

## Run from source

Requirements: Python 3.10+, `ffmpeg` and `ffprobe` on `PATH` or in the project folder, and PySide6 Essentials.

With `uv`, without creating an environment in the project:

```bash
uv run --no-project --with pyside6-essentials python -m fotogramma
```

Optional arguments:

```bash
# Pre-fill the library directory
uv run --no-project --with pyside6-essentials python -m fotogramma --directory /path/to/videos

# Start scanning immediately
uv run --no-project --with pyside6-essentials python -m fotogramma \
  --directory /path/to/videos --scan-on-start
```

## Usage

1. Choose a folder with **Browse** or enter its path.
2. Click **Analyze library**.
3. Use statistics and filters to narrow down candidates.
4. Select one or more videos to reveal available actions.
5. Click a column header to sort and double-click a video for details.
6. Convert directly or export the list for another tool.

The last directory and window geometry are remembered through `QSettings`; scan results are not persisted to disk.

## Tests

```bash
QT_QPA_PLATFORM=offscreen \
  uv run --no-project --with pyside6-essentials \
  python -m unittest discover -s tests -v
```

For worker and real conversion tests with demo fixtures:

```bash
FOTOGRAMMA_DEMO_DIR=/path/to/fixtures QT_QPA_PLATFORM=offscreen \
  uv run --no-project --with pyside6-essentials \
  python -m unittest discover -s tests -v
```

The suite currently contains **36 tests** covering the scanner, Qt models, main window, workers, presets, online search, real HEVC conversion, full validation, backup restoration, and release publishing.

## Build

```bash
uv run --no-project --with pyside6-essentials --with pyinstaller==6.22.3 \
  python deployment/build.py
```

See [`deployment/README.md`](deployment/README.md) for platform-specific packaging instructions.

## CI/CD and releases

![Build & Release](https://github.com/JohnnyTaxs/fotogramma/actions/workflows/build-release.yml/badge.svg)

The GitHub Actions workflow in `.github/workflows/build-release.yml` runs tests and builds on **Linux, Windows, and macOS ARM**. macOS Intel is intentionally excluded. On every `v*` tag it publishes or updates a GitHub release containing all portable ZIP files.

Runner and token details are documented in [CI/CD with GitHub Actions](docs/CICD.md). Public repositories can use GitHub-hosted runners for free.

## Project structure

```text
fotogramma/
  app.py                 Entry point, icon, fonts, and CLI options
  core/scanner.py        Discovery, ffprobe, classification, cancellation
  core/media_tools.py    ffmpeg path in source and frozen builds
  core/conversion.py     Presets, FFmpeg arguments, safe output allocation
  core/verification.py   Full validation and atomic backup replacement
  core/search.py         Title cleanup and external 1337x.to URLs
  ui/main_window.py      Main window and selection actions
  ui/conversion_dialog.py Presets and conversion progress
  ui/models.py           QAbstractTableModel and QSortFilterProxyModel
  ui/workers.py          ScanWorker and ConversionWorker
  ui/theme.py            Cinema Pop QSS
  ui/widgets.py          Native composition bar
runtime.py               Resource paths in source and frozen builds
static/                  Fonts, licenses, and original icon
deployment/              PyInstaller and FFmpeg vendoring
tests/                   unittest suite, including offscreen Qt
docs/                    Architecture, CI/CD, and demo assets
```

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Build and distribution](deployment/README.md)
- [CI/CD with GitHub Actions](docs/CICD.md)
- [Agent guide](AGENTS.md)
- [MIT License](LICENSE)

## Privacy and license

The app opens no server and sends no data online during scanning or conversion. Only the explicit 1337x.to search opens an external URL containing the video title.

**Sora** and **IBM Plex Mono** are distributed with their respective SIL Open Font Licenses, included in `static/fonts/`. The original icon is included in `static/icons/`.

Fotogramma itself is released under the **MIT License**. See [`LICENSE`](LICENSE) for the full text, including the warranty disclaimer.
