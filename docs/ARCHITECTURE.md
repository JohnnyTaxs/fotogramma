# Native architecture

Fotogramma Desktop has no HTTP component. The executable starts an `egui`/`eframe` window backed by OpenGL and talks to FFmpeg only through child processes.

```text
fotogramma (Rust binary)
        |
        v
eframe + egui (glow / OpenGL)
        |
        +--> rfd file dialogs
        +--> TOML settings (directory, window size, defaults)
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

`Cargo.toml` defines a two-crate workspace, version `2.11.0`:

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

`ConversionProfile` defines CRF, preset, pixel format, tune, AQ mode, and audio behavior. The built-in profiles are `max_quality` (veryslow, about 6x the time of `balanced`), `high_quality` (CRF 20, slow, AQ 3: nearly the same quality in a third of the time), `balanced`, and `fast`, plus `custom_profile`. The conversion dialog explains the differences in a collapsible comparison (`profile_comparison`); the times there are indicative.

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

### `crates/core/src/i18n.rs`

Italian and English. `Lang` resolves the setting (`auto`, `it`, `en`) against the system locale; the current language is a process-wide atomic because messages are produced on worker threads too. `tr_str(it, en)` picks a static string and `tr!(it, en, args…)` formats one. Number formats follow the language (`1.234,5` or `1,234.5`). `auto` maps `it*` locales to Italian and everything else to English.

### `crates/core/src/update.rs`

Self-update, the only code that touches the network. `check` reads the latest GitHub release and `pick_release` chooses this platform's archive (`Fotogramma-<System>-<arch>.zip`), accepting only assets under this repository's download URL. `download` streams the archive while hashing it and refuses it unless the SHA-256 equals the entry in `SHA256SUMS.txt`. `stage` unpacks it next to the installed copy (`ditto` on macOS, the zip crate elsewhere) so the final move is a rename, and on macOS checks the code signature. `spawn_swap` starts a detached helper (`sh` on macOS and Linux, `cmd` on Windows) that waits for the app to exit, renames the old copy aside, moves the new one in, restores the old one if that fails, and relaunches. `InstallTarget::detect` is `None` for a development binary, which is never replaced.

The window in `dialogs.rs` shows the release notes, then *Update and restart*, *Later* or *Skip this version* (stored in `settings.toml`). The restart is held back while a scan or conversion runs.

### Content type (in `conversion.rs`)

`MediaKind` is `Anime` or `Live`. `detect_kind(path)` tokenizes the folder names above a file (lowercase, split on non-alphanumerics) and returns `Anime` if any token is an anime word, else `Live` if any is a film/series word; `detect_selection` requires the files of a selection to agree. The dialog (`ConversionDialog::effective_kind`) combines the setting (`auto`, `anime`, `live`) with the detection and applies only the x265 tune: `animation` for anime, none for live action. The audio choice is merged into built-in profiles in `current_profile`, so every preset honours it.

### `crates/app/src/markdown.rs`

A small Markdown renderer for the release notes in the update window: headings, bullet and numbered lists, bold, italic, inline code, code blocks, quotes, rules; links show their text only (the app opens no browser). `parse_blocks` and `parse_inline` are pure and tested. `sanitize` runs first because the notes come from the network and the interface font cannot draw every character: arrows become `->`, check marks `OK`, anything unknown `?`. Bullets are drawn, not typed.

### Menu (`menu.rs`, `native_menu.rs`)

One definition, two renderers. `MenuAction` lists what the menu can do; `menus(native)` says which menu holds what (on macOS Settings, About, Check for Updates and Quit are in the application menu and Help is left to the system, elsewhere they are in Edit, File and Help); `MenuState::enabled` greys out what cannot work (no results yet, nothing selected, a scan or a window in the way). On macOS `NativeMenu` builds the menu bar with `muda` (`init_for_nsapp`, with the Window and Help menus registered with the system), chosen items go into a queue that the next frame reads, and the menu is rebuilt when the language changes. On Windows and Linux `show_bar` draws an `egui::MenuBar` in a top panel and `shortcut_pressed` reads the keys; on macOS the native menu takes the key before the window sees it. `FotogrammaApp::handle_menu_action` carries every action out, and the tests exercise it directly, so the logic is checked on every platform even though the macOS menu bar itself is not drawn by egui.

### Scan cache, queue, export and the command line

`cache::ScanCache` stores the probe result of readable videos keyed by path, with size and modification time as the validity check. `scanner::probe_videos_cached` runs a pool of up to 8 threads that each take the next file, looks each file up in the cache first, and reports how many results came from it; a complete scan prunes entries under the scanned root that were not seen, an interrupted one forgets nothing. `queue::PendingQueue` is written after every file and removed when the queue ends or is stopped, and the app offers to resume a leftover one at startup. `export` renders paths, CSV (a field that starts with `=`, `+`, `-` or `@` is prefixed so a spreadsheet does not run it) or JSON, and computes the bitrate shown in the table. `paths::config_dir` is shared by the app and `fotogramma-cli`, a small binary of `fotogramma-core` with a hand-written argument parser that shares the cache with the app.

### Converted videos

`FotogrammaApp::converted` maps each original to the file made from it (the original's own path when it was replaced); it lives in memory and is cleared by a new scan. `mark_replaced_videos` runs every frame, so rows change as the queue advances: a replaced video is reclassified as HEVC with its new size (and the statistics are recomputed), any success is removed from the selection, and `table::show` shows *Already converted* with the output path in the tooltip. `convertible_in` (Select all) and the export skip converted videos; `conversion_progress` gives the pending and done counts used by the card and the hint.

### Space saved

`ConversionResult` carries `original_size` (read before the conversion, so before any replacement) and `new_size` (read after it). `formatting::SpaceChange` adds up the files that succeeded and gives the saved or added bytes and the signed percentage. `ConversionSession` keeps the total in `space` once the queue ends; the progress dialog shows it, each log line carries the per-file figures, and closing the dialog leaves a line in the status bar.

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

Cinema Pop colors and embedded **Outfit** (a static Regular instance of the variable font, because egui cannot pick a weight and the default would be Thin) / **IBM Plex Mono** fonts (SIL Open Font License, licenses in `static/fonts/`). `Visuals::strong_text_color()` derives from `widgets.active.fg_stroke`; it must stay light or `.strong()` labels become nearly invisible.

### `crates/app/src/settings.rs`

Reads and writes `settings.toml` in `~/Library/Application Support/Fotogramma`, `%APPDATA%\Fotogramma`, or `$XDG_CONFIG_HOME/fotogramma`. The directory, window size, language, accepted codecs, conversion defaults, the user's presets, and the cache switch are stored. Scan results are not in `settings.toml`; the only copy of them is the scan cache described below. Changing the accepted codecs re-classifies the loaded library with `scanner::reclassify` instead of scanning again. The config directory is disabled under `cfg!(test)` so snapshots never touch user files.

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
