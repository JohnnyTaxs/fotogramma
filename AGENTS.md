# AGENTS.md

Operational guide for agents modifying **Fotogramma Desktop** (v2, Rust). The webapp in `h265-check` is a separate project: do not turn this application into a launcher or WebView.

## Invariants

- **Native desktop only** through `egui`/`eframe` with the `glow` (OpenGL) renderer. No HTTP server, Electron, Tauri, Qt, WebView, or external browser. **The only network request is the update check** (`fotogramma_core::update`): HTTPS to `api.github.com` and to this repository's release downloads, nothing about the library is ever sent, it is **opt-in**: the first start asks once (`dialogs::show_update_prompt`) and no request is made until the user says yes; the choice can be changed in Settings, `FOTOGRAMMA_NO_UPDATE_CHECK` switches it off, and it never runs in tests or with `--smoke-test`. Do not add any other request.
- **Default mode never overwrites originals.** Replacement is allowed only after HEVC, duration, stream, full-decode, and final-codec validation, with an atomic backup restored on failure.
- By default `hevc` and `av1` are `compatible`; every other valid codec is `convert`; read failures are `error`. The user can change the accepted codecs in the settings window: the probe keeps the defaults and the app re-applies the list with `reclassify`, so a change never needs a new scan. Verification after a conversion still requires `hevc`.
- Only the first video stream (`v:0`) of `.mp4`, `.mkv`, and `.webm` is analyzed.
- **The interface font cannot draw everything.** In the app a missing glyph is drawn as an empty box (egui's default `Tofu` policy); only the test harness turns it into a panic, so a test is how you find one. Outfit has accents, `·`, `…`, `×`, bullets, `← ↑ → ↓` and `✓`; it has no `✔`, `⇒`, `★`, emoji, or CJK. Add a new symbol only after rendering it in a test; text from outside, such as release notes, goes through `markdown::sanitize`. File names in Japanese, Chinese or Korean are drawn with a system font that `theme::ensure_fallback_fonts` loads only when a scan finds such names (Korean on macOS is 55 MB).
- **x265 has no `film` tune** (that is x264): never pass `-tune film`; the valid tunes here are `animation` and `grain`. Content type (`MediaKind` in `conversion.rs`) only chooses the tune: anime → `animation`, live action → none. Detection reads the folders only, and anime anywhere in the path wins.
- **Conversions are never silently degraded.** An MP4 target with subtitles MP4 cannot hold falls back to MKV (`container_for`), an HDR source is never processed in 8 bits (`profile_for_video`), and a result that saves less than the chosen minimum is dropped with the original kept, and a file clearly heading for a result that is too big is stopped on the way (`heading_for_too_big`, from 10% on, default on, switchable). HEVC in MP4 is tagged `hvc1` (FFmpeg's default `hev1` does not play in QuickTime or Safari). Converted files keep the original's modification time.
- **Menu rules.** A new user-facing action goes in `MenuAction` (label in both languages, shortcut, enabled rule) and is carried out in `handle_menu_action`, so the macOS menu bar and the in-window menu stay the same. The Edit menu has no Cut, Copy or Paste: a native item would take Cmd+C and Cmd+V from the text fields, and no shortcut may use Cmd+A, C, V, X or Z (a test checks it). While any window is open the menu does nothing underneath it. Quit goes through the normal close handling.
- No CDN or online service is required at runtime; if GitHub is unreachable the app works exactly the same.
- **Updates are verified before they are installed.** The package must be an asset of this repository's release, its SHA-256 must match `SHA256SUMS.txt` (published by `create_release.py`), and on macOS `codesign --verify` must pass before the swap. A development binary (`cargo run`) is never replaced. The swap runs in a helper after the app quits and restores the old copy if the move fails: a shell script on macOS and Linux, and on Windows a copy of the program itself (`fotogramma.exe --apply-update`, run from the temp folder, `apply_swap`). Never a hidden `.cmd` or `ping` loop: that is the pattern Windows Defender flags.
- **Two languages: Italian and English.** Every user-visible string goes through `tr_str` or `tr!` with both texts (never a bare literal), including messages built in `core` and on worker threads. The language is process-wide (`i18n::set_lang`); Italian is the default, `auto` follows the OS through `sys-locale`, and the Settings window can override it. Prefer "Label: {}" over "{} items" so no plural form is needed. Tests fix the system locale to `it-IT`; UI tests take the `lang_guard` lock, and English output is tested in `crates/core/tests/english.rs`.
- `settings.toml` stores the directory, window size, language, accepted codecs, conversion defaults (replace original, profile, container, content type, output folder, minimum saving, keep awake), the user's presets, the cache switch, and the scan filter (included extensions, excluded name words; matched on the file name only). Scan results are never in it. The only copy of probe results is `scan-cache.json` (readable videos only, keyed by path, size and mtime, written atomically, switchable and clearable), and `queue.json` exists only while a queue runs. Do not store anything else about the library.
- Build **onedir** for each OS/architecture, never onefile and never cross-compiled. macOS also ships a drag-to-Applications `.dmg` (`dmgbuild`, background in `deployment/dmg/`); the ZIP stays because the updater installs it.
- The packaged app must stay within **45 MB** with a ZIP (and the DMG) within **18 MB**.

## Commands

```bash
# Run from source (requires ffmpeg/ffprobe on PATH or FOTOGRAMMA_FFMPEG_DIR)
cargo run --release -p fotogramma

# Full test suite
cargo test --workspace

# Update the four egui_kittest snapshots after an intentional layout change
UPDATE_SNAPSHOTS=1 cargo test -p fotogramma snapshot

# Regenerate the README screenshots in docs/assets (English, a demo library of Blender's open movies, never real files)
cargo test -p fotogramma readme_screenshots -- --ignored

# Minimal FFmpeg (macOS/Linux), then package and enforce the size budget
deployment/ffmpeg/build_minimal.sh
python3 deployment/build_rust.py

# Static checks
cargo check --workspace && cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

## Architecture

| File | Rules |
| --- | --- |
| `crates/core/src/scanner.rs` | Discovery, `Popen`-equivalent probing, classification, cancellation. Must remain UI-independent. |
| `crates/core/src/media_tools.rs` | ffmpeg/ffprobe resolution in source and packaged builds; `DYLD_LIBRARY_PATH` only on macOS. |
| `crates/core/src/conversion.rs` | Profiles, FFmpeg arguments, and `MediaKind` detection. Use `-n`, `-map 0`, and non-overwriting output. |
| `crates/core/src/i18n.rs` | `Lang`, the process-wide language, `tr_str` and `tr!`. No other module may hardcode user-visible text in one language. |
| `crates/app/src/menu.rs` | The application menu, defined once: `MenuAction`, labels (both languages), shortcuts, which menus hold what, and when an entry is enabled. Drawn in the window on Windows and Linux. |
| `crates/app/src/native_menu.rs` | The macOS menu bar, built with `muda` from `menu.rs`. macOS only; no GTK anywhere. |
| `crates/core/src/cache.rs` | Scan cache. A hit needs the same path, size and mtime; unreadable files are never cached. |
| `crates/core/src/queue.rs` | The running queue on disk, so it can be resumed; removed when the queue ends or is stopped. |
| `crates/core/src/export.rs` | Paths, CSV (formulas defused) and JSON lists, bitrate, and `read_paths` for importing a list back. An imported list is probed again (`worker::spawn_import`), never trusted, and never prunes the cache. |
| `crates/core/src/paths.rs` | The configuration folder, shared by the app and the CLI. |
| `crates/core/src/bin/fotogramma-cli.rs` | The command line tool: no window, no network, same engine. Keep it free of UI code. |
| `crates/core/src/update.rs` | Release check, verified download, staging, and the swap helper. The only module that uses the network. |
| `crates/core/src/verification.rs` | Codec/duration/stream validation, full decode, and atomic backup replacement. |
| `crates/app/src/app.rs` | Entry state, layout, selection, scan flow, smoke test, four snapshots. |
| `crates/app/src/markdown.rs` | Renders the release notes (headings, lists, bold, italic, code). Text from the network is sanitized first. |
| `crates/app/src/mascot.rs` | The easter egg on the progress bar: she runs towards a TV, which turns on only when the conversion has finished. Frames in `deployment/mascot.png` and `deployment/tv.png`, original art made by `deployment/make_mascot.py` (never bundle someone else's character). Switchable in Settings; tests fix the frame. |
| `crates/app/src/icon.rs` | Apply the bundled icon to the macOS Dock at startup. |
| `crates/app/src/table.rs` | Virtualized table, filters, sorting, selection, exact column widths. |
| `crates/app/src/dialogs.rs` | Preferences window, conversion settings, command preview, progress, results, close confirmation. |
| `crates/app/src/results.rs` | The table of a queue's results (outcome, size before and after, saving, total), opened from the progress window. It covers every batch of the queue (`ConversionSession::history`). |
| `crates/app/src/worker.rs` | Scan and conversion threads; communicate only through channels. |
| `crates/app/src/theme.rs` | Cinema Pop colors and embedded Outfit/IBM Plex Mono. Do not introduce HTML/CSS or a second theme. |
| `crates/app/src/settings.rs` | TOML preferences and defaults (accepted codecs, conversion defaults); config directory disabled under `cfg!(test)`. |
| `crates/app/src/platform.rs` | Open a folder in the system file manager. |

Data flow: directory → `discover_videos_filtered` → `probe_videos` → `mpsc` → app state → `TableBuilder`. Starting a scan or an import clears the previous library first (`clear_results`), so nothing acts on stale rows.

## Threads and shutdown

- Never update widgets from a worker thread: send a message and call `ctx.request_repaint()`.
- Keep the `Receiver` and cancellation `Arc<AtomicBool>` alive for the job's duration.
- Cancellation kills the `ffprobe` child; conversion kills the `ffmpeg` child and reports `cancelled`.
- In the close path, ask for confirmation if a job is active and wait for it before exiting.
- Conversion merges `stderr` into `stdout`, reads `-progress pipe:1`, and closes the pipe after `wait()`.
- Do not run parallel conversions: the sequential queue avoids CPU and disk saturation. The progress window can be minimized to a bar (`session_minimized`); batches added meanwhile wait in `FotogrammaApp::waiting` and start one after the other (`advance_queue`), Stop drops them, and only the running batch is in `queue.json`. A video in a running or waiting batch cannot be chosen again (`is_queued`), and a scan or a new folder stays off while a conversion runs.
- Replacement must always use `allocate_partial_path`, `validate_conversion_output`, and `replace_verified_output`. Never rename the original before these steps and never delete the backup before the final check. WebM replacement is unsupported.

## Table contract

The table uses `egui_extras::TableBuilder` with virtualized rows, native sorting, and exact column widths. Adding a column means updating the header list, cell rendering, sort values, tests, and layout math. Category filters and search must remain case-insensitive across name, path, and codec.

## Snapshots and accessibility

- Four accepted PNGs live in `crates/app/tests/snapshots/`: main window and conversion dialog at 1280×820 and 1050×680.
- `egui_kittest` writes `*.new.png` on mismatch and keeps the previous `*.old.png`; accept deliberately with `UPDATE_SNAPSHOTS=1` and delete the backups.
- Every interactive control needs an accessible label (`labelled_by`); the accessibility check must pass.
- `Visuals::strong_text_color()` reuses `widgets.active.fg_stroke`; keep it light or `.strong()` text becomes invisible.
- The conversion modal must fit 1050×680: capped width, scrollable body, pinned action row.

## Build, CI and releases

Before touching `deployment/`, `.github/workflows/`, packaging, or a release, load the `release-packaging` skill (`.agents/skills/release-packaging/SKILL.md`): the FFmpeg component list, per-OS collection and signing, and the CI and release rules live there. Do not commit `target/`, `build/`, `dist/`, `deployment/vendor/`, or screenshots containing real user data.

## Checklist

1. No browser/WebView/server dependency was introduced.
2. `cargo test --workspace` is green and snapshots are intentional.
3. GUI behavior is verified with offscreen `egui_kittest` snapshots, not just logic tests.
4. Five fixtures are scanned: two convert, two compatible, one error.
5. A short H.264 is converted and verified as `hevc` without modifying the original; replacement is tested with a temporary copy.
6. The moved macOS bundle starts with `PATH=/usr/bin:/bin`, scans, converts, and shuts down.
7. `cargo fmt --check`, `cargo check --workspace`, `cargo clippy ... -D warnings`, and `git diff --check` pass.
8. README, `docs/`, this guide, and the `release-packaging` skill are updated if architecture, commands, or packaging change.
9. The size budget passes; report the measured app and ZIP sizes.
