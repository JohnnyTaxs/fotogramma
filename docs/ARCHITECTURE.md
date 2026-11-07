# Native architecture

Fotogramma Desktop has no HTTP component. The executable starts `QApplication` and a native `MainWindow` directly.

```text
fotogramma.__main__
        |
        v
QApplication + MainWindow
        |
        +--> QFileDialog / QSettings
        |
        +--> ScanWorker (QThread)
                |
                +--> discover_videos
                +--> probe_videos (up to 4 ffprobe processes)
                |
                v
        Qt signals in the GUI thread
                |
                v
VideoTableModel --> VideoFilterProxy --> QTableView
        |
        +--> selected videos --> external search or ConversionWorker
                                      |
                                      +--> ffmpeg (new .h265 files)
```

## Modules

### `fotogramma/app.py`

- Creates `QApplication` and loads fonts through `resource_path`.
- Applies the Cinema Pop QSS theme and original application icon.
- Exposes `--directory`, `--scan-on-start`, and `--smoke-test` for automated verification.
- Closes smoke-test runs only after the worker has finished.

### `core/scanner.py`

Contains the video engine, independent of Qt.

- `discover_videos`: deterministic traversal and `os.walk` warnings.
- `probe_video`: `Popen` with a 45-second timeout, JSON metadata, and cancellation.
- `probe_videos`: up to four processes per batch, with no second queued wave.
- `get_ffprobe_path`: prefers the bundled binary in frozen applications.
- `ffprobe_environment`: adds `lib` to `DYLD_LIBRARY_PATH` in frozen macOS builds.

### `core/media_tools.py`

`get_ffmpeg_path()` resolves `ffmpeg` in source and frozen builds, preferring the bundled binary. `ffprobe_environment()` is shared with conversion processes so they use the included native libraries.

### `core/conversion.py`

`ConversionProfile` defines CRF, preset, pixel format, tune, AQ mode, and audio behavior. Available profiles are `max_quality`, `balanced`, and `fast`, plus `custom_profile` for manual configuration.

`build_ffmpeg_args()` generates:

- `-n` to prevent overwriting;
- `-map 0` for all streams;
- `libx265` with the selected CRF, preset, and pixel format;
- audio `copy` or AAC with a selected bitrate;
- subtitles and data `copy`;
- original metadata and chapters;
- `+faststart` only for MP4;
- `-progress pipe:1` for native progress reporting.

`allocate_output_path()` adds `.h265` and a numeric suffix when the destination exists. `allocate_partial_path()` creates a same-filesystem temporary file used only after full validation and preserves the original MKV/MP4 extension.

### `core/verification.py`

Verified replacement has two separate phases:

1. `validate_conversion_output()` checks HEVC, duration within max(1 second, 1%), matching stream counts, and a full decode using `ffmpeg -v error -i output -f null -`.
2. `replace_verified_output()` renames the original to `.fotogramma-backup`, moves the `.partial` over the original path, checks the codec again, and only then deletes the backup.

If any test fails, no replacement occurs. If the final check fails after renaming, the backup restores the original. WebM replacement is unsupported.

### `core/search.py`

Cleans release tags from the title and builds `https://1337x.to/search/<title+H.265>/1/`, including the required first-page suffix. `QDesktopServices.openUrl()` opens the external browser only after an explicit user action; no network request is made by the app itself.

### `ui/workers.py`

`ScanWorker` runs outside the GUI thread and emits:

- `progress(processed, total, current_file)` with `total = -1` during discovery;
- `scanCompleted(videos, warnings, cancelled)`;
- `failed(message)` for unrecoverable errors.

`ConversionWorker` processes a sequential queue to avoid CPU and disk saturation. It merges `stderr=STDOUT`, reads `out_time_ms` to compute progress relative to video duration, and always closes the output stream after `wait()`.

### `ui/models.py`

`VideoTableModel` exposes six native columns: Video, Codec, Duration, Resolution, Size, and Action.

Custom roles:

- `INFO_ROLE`: full `VideoInfo` object;
- `SORT_ROLE`: typed value used for sorting;
- `CATEGORY_ROLE`: category used by the proxy.

`VideoFilterProxy` applies category filtering and case-insensitive search across name, path, and codec. Numeric sorting uses the custom role, so size and duration are not sorted as strings.

### `ui/main_window.py`

Handles:

- directory selection and validation;
- clickable statistics and filters;
- progress and cancellation;
- partial-result warnings;
- double-click details;
- export through `QFileDialog`;
- selection-based online search and conversion;
- close confirmation when a job is active.

`QSettings` stores only the directory and geometry. Videos remain in memory while the app is open.

## UI states

The `QStackedWidget` contains:

1. the initial page with a codec matrix;
2. the results page;
3. the error page with Retry.

Controls are disabled during scanning and progress is shown in the command bar. Interrupted files are not classified as errors.

## Frozen builds

`runtime.py` uses `sys._MEIPASS` to locate assets and binaries. The onedir bundle contains:

- the Python interpreter;
- PySide6 Essentials and required Qt plugins;
- `static/` with fonts and licenses;
- `ffmpeg`, `ffprobe`, and native dependencies;
- the `fotogramma` package and `runtime.py`.

The bundle can be moved. Verification extracts it in another directory and starts it with `PATH=/usr/bin:/bin`, avoiding system Python and Homebrew.

## Testing

- Engine tests with mocks for `Popen`, `os.walk`, and concurrency.
- Model/proxy tests with offscreen `QApplication`.
- `MainWindow` tests for statistics, filters, selection, warnings, and the conversion dialog.
- Worker tests on five real fixtures via `FOTOGRAMMA_DEMO_DIR`.
- Real short H.264 conversion verified as `hevc`, with the original unchanged.
- Preset, AAC/faststart argument, selection-action, command-preview, and online URL tests.
- Full validation, successful replacement, failed-validation preservation, and backup restoration tests.
- GitHub release client tests with a local HTTP server for create, retrieve, and asset replacement, with no token in logs.
- Native screenshots captured with `QWidget.grab()`, not browser or HTML screenshots.
