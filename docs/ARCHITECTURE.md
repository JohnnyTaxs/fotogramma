# Native architecture

Fotogramma Desktop has no HTTP component. The executable starts an `egui`/`eframe` window backed by OpenGL and talks to FFmpeg only through child processes.

```text
fotogramma (Rust binary)
        |
        v
eframe + egui (glow / OpenGL)
        |
        +--> rfd file dialogs
        +--> TOML settings (directory and window size)
        |
        +--> scan thread --------------------------------+
        |       discover_videos                          |
        |       probe_videos (up to 4 ffprobe processes) |
        |       mpsc channel --> GUI thread              |
        |                                                |
        +--> conversion thread (sequential queue)        |
                build_ffmpeg_args                        |
                ffmpeg -progress pipe:1                  |
                validate_conversion_output               |
                replace_verified_output                  |
                mpsc channel --> GUI thread              |
```

## Workspace

`Cargo.toml` defines a two-crate workspace, version `2.1.0`:

| Crate | Responsibility |
| --- | --- |
| `fotogramma-core` | Discovery, probing, classification, conversion profiles, FFmpeg arguments, verification, formatting. No UI dependency. |
| `fotogramma` | `eframe` window, table, dialogs, theme, settings, worker threads, platform helpers. |

Release profile: `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`, `opt-level = "z"`. The executable is about 3.3 MB.

## Modules

### `crates/core/src/scanner.rs`

- `discover_videos`: deterministic recursive traversal of `.mp4`, `.mkv`, and `.webm`; collects warnings instead of aborting on unreadable directories.
- `probe_video`: runs `ffprobe` with a 45-second timeout on the first video stream (`v:0`), then reads duration, size, resolution, and stream categories.
- `probe_videos`: up to four concurrent `ffprobe` processes, cancellable through an `AtomicBool`; a cancelled probe kills the child with `Popen`-equivalent semantics.
- Classification: `hevc` and `av1` are compatible; any other readable codec is `convert`; probe failures are `error`.

### `crates/core/src/media_tools.rs`

`find_tool()` searches, in order: `FOTOGRAMMA_FFMPEG_DIR`, the executable folder, `bin/` beside it, the macOS `Resources/bin` folder, the working directory, then `PATH`.

On macOS, `tool_environment()` points `DYLD_LIBRARY_PATH` at the tool folder when shared libraries sit next to the binary. Packaged builds use `@loader_path` install names instead, so no environment variable is required. `prepare_command()` applies the same environment to every child process.

### `crates/core/src/conversion.rs`

`ConversionProfile` defines CRF, preset, pixel format, tune, AQ mode, and audio behavior. The built-in profiles are `max_quality`, `balanced`, and `fast`, plus `custom_profile`.

`build_ffmpeg_args()` generates:

- `-n` to prevent overwriting;
- `-map 0` for all streams;
- `libx265` with the selected CRF, preset, and pixel format;
- audio `copy` or AAC at the selected bitrate;
- subtitles and data `copy`;
- original metadata and chapters;
- `+faststart` only for MP4;
- `-progress pipe:1` for native progress reporting.

`allocate_output_path()` adds `.h265` and a numeric suffix when the destination exists. `allocate_partial_path()` creates a same-filesystem temporary file used only after full validation and preserves the original MKV/MP4 extension; WebM replacement is rejected.

### `crates/core/src/verification.rs`

Verified replacement has two separate phases:

1. `validate_conversion_output()` checks HEVC, duration within max(1 second, 1%), matching stream counts, and a full decode using `ffmpeg -v error -i output -f null -`.
2. `replace_verified_output()` renames the original to `.fotogramma-backup`, moves the `.partial` over the original path, checks the codec again, and only then deletes the backup.

If any test fails, no replacement occurs. If the final check fails after renaming, the backup restores the original. On Windows, the failed output is removed before restoring because `rename` cannot replace an existing file.

### `crates/app/src/app.rs`

Owns the window state: directory, scan lifecycle, filters, selection, sorting, warnings, conversion dialog, and progress session. All widget interaction happens in the GUI thread. Smoke-test mode closes the window after startup or after the scan completes.

Four `egui_kittest` snapshot tests render the main window and conversion dialog at 1280×820 and 1050×680.

### `crates/app/src/table.rs`

A virtualized `egui_extras::TableBuilder` with six columns: Video, Codec, Duration, Resolution, Size, and Action. Sorting uses typed values, so size and duration never sort as strings. Selection supports click, Ctrl/Cmd toggle, and Shift ranges; category filters and case-insensitive search match name, path, and codec.

### `crates/app/src/worker.rs`

`spawn_scan()` and `spawn_conversion()` return `mpsc::Receiver`s and request a repaint after every message. The scan worker sends `Progress`, `Completed`, and `Failed`. The conversion worker sends `FileStarted`, `Progress`, `VerificationStarted`, `FileCompleted`, and `ConversionCompleted`.

Conversions run sequentially to avoid saturating CPU and disk. `stderr` is merged into `stdout`, `out_time_ms` is parsed for progress, and the pipe is closed after `wait()`.

### `crates/app/src/dialogs.rs`

Profile selection, custom CRF/preset/pixel format/tune/AQ controls, audio mode, container, destination, three-row FFmpeg command preview, progress, log, per-file results, and the close confirmation. The modal is capped in width and height and scrolls its body while pinning the action buttons.

### `crates/app/src/theme.rs`

Cinema Pop colors and embedded **Sora** / **IBM Plex Mono** fonts (SIL Open Font License, licenses in `static/fonts/`). `Visuals::strong_text_color()` derives from `widgets.active.fg_stroke`; it must stay light or `.strong()` labels become nearly invisible.

### `crates/app/src/settings.rs`

Reads and writes `settings.toml` in `~/Library/Application Support/Fotogramma`, `%APPDATA%\Fotogramma`, or `$XDG_CONFIG_HOME/fotogramma`. Only the directory and window size are stored; scan results are never persisted. The config directory is disabled under `cfg!(test)` so snapshots never touch user files.

## UI states

The window shows the initial composition summary, the results table after a scan, and inline progress while scanning. The conversion dialog is a modal with a fixed action row. Controls are disabled when no video is selected or while a job is active.

## Packaging

`deployment/build_rust.py` runs on the target operating system:

- macOS: `Fotogramma.app` with `Contents/MacOS/fotogramma`, `Contents/Resources/bin`, `Contents/Resources/lib`, `Info.plist`, and icon; ad-hoc signed; ZIP created with `ditto` and the extracted bundle verified with `codesign`.
- Linux: `Fotogramma/` with the binary, `bin/ffmpeg`, `bin/ffprobe`, and `lib/` shared objects; the tools carry an `$ORIGIN/../lib` runpath.
- Windows: `Fotogramma/` with `fotogramma.exe`, `bin/ffmpeg.exe`, `bin/ffprobe.exe`, and the required DLLs.

The build fails when the application exceeds 45 MB or the ZIP exceeds 18 MB.

## Testing

- Core unit tests for discovery, probing, classification, presets, arguments, allocation, verification tolerance, and formatting.
- App unit tests for settings, selection, filters, and dialog outcomes.
- Four `egui_kittest` snapshots, accepted with `UPDATE_SNAPSHOTS=1 cargo test`.
- Two real FFmpeg pipeline tests for copy mode and atomic replacement; they skip when no tools are installed.
- Packaging round-trip: moved bundle, clean `PATH`, bundled tools, `codesign --verify --deep --strict`.
