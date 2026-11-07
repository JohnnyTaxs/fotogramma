# AGENTS.md

Operational guide for agents modifying **Fotogramma Desktop**, the native project. The webapp in `h265-check` is a separate project: do not turn this application into a launcher or WebView.

## Invariants

- **Native Qt widgets only** through PySide6 Essentials. No HTTP server, Electron, Tauri, or Qt WebEngine. An external browser is allowed only for the user-requested 1337x.to search.
- **Default mode never overwrites originals.** Replacement is allowed only after HEVC, duration, stream, full-decode, and final-codec validation, with an atomic backup restored on failure.
- `hevc` and `av1` are `compatible`; every other valid codec is `convert`; read failures are `error`.
- Only the first video stream (`v:0`) of `.mp4`, `.mkv`, and `.webm` is analyzed.
- No CDN or online service is required at runtime.
- `QSettings` stores only directory and geometry, not scan results.
- Build **onedir** for each OS/architecture, never onefile and never cross-compiled.

## Commands

```bash
# Run from source
uv run --no-project --with pyside6-essentials python -m fotogramma

# Full test suite
QT_QPA_PLATFORM=offscreen \
  uv run --no-project --with pyside6-essentials \
  python -m unittest discover -s tests -v

# Build
uv run --no-project --with pyside6-essentials --with pyinstaller==6.22.3 \
  python deployment/build.py

# Static checks
python3 -m compileall -q fotogramma runtime.py deployment
git diff --check
```

Do not install the full `PySide6` package: it includes WebEngine and unnecessary dependencies. Always use **PySide6-Essentials**.

## Architecture

| File | Rules |
| --- | --- |
| `fotogramma/app.py` | Entry point, fonts, icon, and smoke-test timers. Never add a server. |
| `fotogramma/core/scanner.py` | Discovery, Popen, classification, and cancellation. Must remain Qt-independent. |
| `fotogramma/ui/workers.py` | `ScanWorker` and `ConversionWorker`. No UI operations in worker threads. |
| `fotogramma/ui/main_window.py` | All interactions happen in the GUI thread, including multi-selection. |
| `fotogramma/core/conversion.py` | Profiles and FFmpeg arguments. Use `-n`, `-map 0`, and non-overwriting output. |
| `fotogramma/core/media_tools.py` | ffmpeg resolution in source and frozen builds. |
| `fotogramma/core/search.py` | External 1337x.to URLs only; no integrated network requests. |
| `fotogramma/core/verification.py` | Codec/duration/stream validation, full decode, and atomic backup replacement. |
| `fotogramma/ui/conversion_dialog.py` | Profiles, command preview, and progress. Keep controls in the GUI thread. |
| `fotogramma/ui/models.py` | Model/proxy for large libraries, custom roles, and badges. |
| `fotogramma/ui/theme.py` | Cinema Pop QSS; do not introduce HTML/CSS or a second theme. |
| `runtime.py` | `resource_path` for assets in source and frozen builds. |
| `deployment/` | Build, download, and vendoring. Do not rename it `packaging`, which would conflict with the Python package. |

Data flow: directory → `discover_videos` → `probe_videos` → Qt signals → model → proxy → `QTableView`.

## Threads and shutdown

- Never update widgets from `ScanWorker.run()`: use `progress`, `scanCompleted`, and `failed`.
- Keep a reference to each worker to prevent destruction while running.
- Cancellation uses `threading.Event`; `probe_video` terminates the process with `Popen.kill()`.
- In `closeEvent`, ask for confirmation if a job is active and wait for the worker before accepting shutdown.
- `ConversionWorker` uses `-progress pipe:1` and `stderr=STDOUT` to avoid deadlocks; always close `process.stdout` after `wait()`.
- Do not run parallel conversions: the sequential queue reduces CPU and disk saturation. `fileStarted`, `progress`, `fileCompleted`, and `conversionCompleted` update the UI only through signals.
- Replacement must always use `allocate_partial_path`, `validate_conversion_output`, and `replace_verified_output`. Never rename the original before these steps and never delete the backup before the final check. WebM replacement is unsupported.
- Do not call Qt GUI methods from worker tests without `QApplication` when using widgets or `QPixmap`; GUI tests use `QT_QPA_PLATFORM=offscreen`.

## Model contract

`INFO_ROLE`, `SORT_ROLE`, and `CATEGORY_ROLE` are internal model dependencies. Update the proxy and tests when changing them. `QSortFilterProxyModel.set_category` and `set_query` must call `invalidate()`; do not use the deprecated `invalidateFilter()`.

The table uses native header sorting, not HTML pagination. For new columns update `HEADERS`, `data`, `_sort_value`, tests, and layout.

## Build and assets

- PyInstaller includes `static/`, `ffmpeg`, and `ffprobe` through `deployment/Fotogramma.spec`.
- On macOS, `ffprobe_environment()` uses `lib` through `DYLD_LIBRARY_PATH`.
- `deployment/prepare_ffprobe.py` verifies with `DYLD_PRINT_LIBRARIES` that no Homebrew library is loaded by `ffmpeg` or `ffprobe`.
- Windows and Linux download both binaries from static archives without extracting arbitrary paths.
- Do not commit `build/`, `dist/`, `deployment/vendor/`, `.venv/`, or screenshots containing real user data.

## CI/CD and releases

The workflow lives at `.github/workflows/build-release.yml` and runs tests and builds on Linux (`ubuntu-latest` with `node:22-bookworm`), Windows (`windows-latest`), and macOS ARM (`macos-15`). Do not add macOS Intel or Forgejo-specific runner configuration. On Linux, install PySide6 Essentials with pip, not the unavailable `python3-pyside6` apt package.

`deployment/create_release.py` publishes or updates a GitHub release through `GITHUB_API_URL` and `GITHUB_REPOSITORY`, replacing same-name assets instead of duplicating them. Never print `GITHUB_TOKEN` or `FORGEJO_TOKEN`.

After changing the workflow or release client, run `tests.test_create_release`. Release tags follow SemVer (`v1.0.0`).

## Checklist

1. No browser/WebView dependency was introduced.
2. Unit tests are updated and green.
3. GUI behavior is verified with offscreen Qt screenshots, not just logic tests.
4. At least five fixtures are scanned: two convert, two compatible, one error; a short H.264 is converted and verified as `hevc` without modifying the original.
5. The moved build starts with `PATH=/usr/bin:/bin`; scanning and conversion use the bundled `ffmpeg`.
6. `git diff --check` and `compileall` pass.
7. README, `docs/`, and this guide are updated if architecture or commands change.
8. Packaging changes are tested by moving the build to another directory, starting it with a clean `PATH`, scanning, converting, and shutting down.
