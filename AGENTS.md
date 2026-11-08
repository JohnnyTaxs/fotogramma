# AGENTS.md

Operational guide for agents modifying **Fotogramma Desktop** (v2, Rust). The webapp in `h265-check` is a separate project: do not turn this application into a launcher or WebView.

## Invariants

- **Native desktop only** through `egui`/`eframe` with the `glow` (OpenGL) renderer. No HTTP server, Electron, Tauri, Qt, or WebView. An external browser is allowed only for the user-requested 1337x.to search.
- **Default mode never overwrites originals.** Replacement is allowed only after HEVC, duration, stream, full-decode, and final-codec validation, with an atomic backup restored on failure.
- `hevc` and `av1` are `compatible`; every other valid codec is `convert`; read failures are `error`.
- Only the first video stream (`v:0`) of `.mp4`, `.mkv`, and `.webm` is analyzed.
- No CDN or online service is required at runtime.
- `settings.toml` stores only the directory and window size, not scan results.
- Build **onedir** for each OS/architecture, never onefile and never cross-compiled.
- The packaged app must stay within **45 MB** with a ZIP within **18 MB**.

## Commands

```bash
# Run from source (requires ffmpeg/ffprobe on PATH or FOTOGRAMMA_FFMPEG_DIR)
cd rust
cargo run --release -p fotogramma

# Full test suite
cd rust
cargo test --workspace

# Update the four egui_kittest snapshots after an intentional layout change
cd rust
UPDATE_SNAPSHOTS=1 cargo test -p fotogramma snapshot

# Minimal FFmpeg (macOS/Linux), then package and enforce the size budget
deployment/ffmpeg/build_minimal.sh
python3 deployment/build_rust.py

# Static checks
cd rust && cargo check --workspace && cargo fmt --check
git diff --check
```

Windows builds use `deployment/ffmpeg/build_minimal_windows.sh` inside MSYS2 MINGW64. Never substitute the full Homebrew/community FFmpeg archive: it breaks the size budget.

Linux and Windows build x265 with `deployment/ffmpeg/build_x265.sh` (8-bit + 10-bit only, static C++ runtime) instead of installing the distribution package; macOS uses Homebrew's arm64 x265.

## Architecture

| File | Rules |
| --- | --- |
| `rust/crates/core/src/scanner.rs` | Discovery, `Popen`-equivalent probing, classification, cancellation. Must remain UI-independent. |
| `rust/crates/core/src/media_tools.rs` | ffmpeg/ffprobe resolution in source and packaged builds; `DYLD_LIBRARY_PATH` only on macOS. |
| `rust/crates/core/src/conversion.rs` | Profiles and FFmpeg arguments. Use `-n`, `-map 0`, and non-overwriting output. |
| `rust/crates/core/src/verification.rs` | Codec/duration/stream validation, full decode, and atomic backup replacement. |
| `rust/crates/core/src/search.rs` | External 1337x.to URLs only; no integrated network requests. |
| `rust/crates/app/src/app.rs` | Entry state, layout, selection, scan flow, smoke test, four snapshots. |
| `rust/crates/app/src/table.rs` | Virtualized table, filters, sorting, selection, exact column widths. |
| `rust/crates/app/src/dialogs.rs` | Conversion settings, command preview, progress, results, close confirmation. |
| `rust/crates/app/src/worker.rs` | Scan and conversion threads; communicate only through channels. |
| `rust/crates/app/src/theme.rs` | Cinema Pop colors and embedded Sora/IBM Plex Mono. Do not introduce HTML/CSS or a second theme. |
| `rust/crates/app/src/settings.rs` | TOML preferences; config directory disabled under `cfg!(test)`. |
| `rust/crates/app/src/platform.rs` | Open a folder or the external browser. |

Data flow: directory → `discover_videos` → `probe_videos` → `mpsc` → app state → `TableBuilder`.

## Threads and shutdown

- Never update widgets from a worker thread: send a message and call `ctx.request_repaint()`.
- Keep the `Receiver` and cancellation `Arc<AtomicBool>` alive for the job's duration.
- Cancellation kills the `ffprobe` child; conversion kills the `ffmpeg` child and reports `cancelled`.
- In the close path, ask for confirmation if a job is active and wait for it before exiting.
- Conversion merges `stderr` into `stdout`, reads `-progress pipe:1`, and closes the pipe after `wait()`.
- Do not run parallel conversions: the sequential queue avoids CPU and disk saturation.
- Replacement must always use `allocate_partial_path`, `validate_conversion_output`, and `replace_verified_output`. Never rename the original before these steps and never delete the backup before the final check. WebM replacement is unsupported.

## Table contract

The table uses `egui_extras::TableBuilder` with virtualized rows, native sorting, and exact column widths. Adding a column means updating the header list, cell rendering, sort values, tests, and layout math. Category filters and search must remain case-insensitive across name, path, and codec.

## Snapshots and accessibility

- Four accepted PNGs live in `rust/crates/app/tests/snapshots/`: main window and conversion dialog at 1280×820 and 1050×680.
- `egui_kittest` writes `*.new.png` on mismatch and keeps the previous `*.old.png`; accept deliberately with `UPDATE_SNAPSHOTS=1` and delete the backups.
- Every interactive control needs an accessible label (`labelled_by`); the accessibility check must pass.
- `Visuals::strong_text_color()` reuses `widgets.active.fg_stroke`; keep it light or `.strong()` text becomes invisible.
- The conversion modal must fit 1050×680: capped width, scrollable body, pinned action row.

## Build and assets

- `deployment/ffmpeg/build_minimal.sh` and `build_minimal_windows.sh` define the only supported FFmpeg component list. Adding a codec or filter must be justified by a real app path and re-measured against the budget.
- `deployment/ffmpeg/collect_macos.py` rewrites install names to `@loader_path`, re-signs every Mach-O file, and verifies relocation.
- `deployment/ffmpeg/collect_linux.py` sets an `$ORIGIN/../lib` runpath with `patchelf`.
- macOS archives use `ditto -c -k --sequesterRsrc --keepParent`; `deployment/build_rust.py` extracts the ZIP again and runs `codesign --verify --deep --strict` before publishing.
- The macOS `Info.plist` embeds the version from `rust/Cargo.toml`.
- Windows embeds the icon and version through `rust/crates/app/build.rs` (`winresource`).
- Do not commit `rust/target/`, `build/`, `dist/`, `deployment/vendor/`, or screenshots containing real user data.

## CI/CD and releases

`.github/workflows/build-release.yml` runs on Linux, Windows, and macOS ARM:

- builds the minimal FFmpeg on each platform;
- runs `cargo test --workspace --locked`;
- reruns the real pipeline tests against the freshly built tools;
- packages with `deployment/build_rust.py` and enforces the size budget;
- on a `v*` tag, publishes through `deployment/create_release.py`.

Do not add macOS Intel or Forgejo-specific runner configuration. Never print `GITHUB_TOKEN` or `FORGEJO_TOKEN`. Release tags follow SemVer (`v2.0.0`).

## Checklist

1. No browser/WebView/server dependency was introduced.
2. `cargo test --workspace` is green and snapshots are intentional.
3. GUI behavior is verified with offscreen `egui_kittest` snapshots, not just logic tests.
4. Five fixtures are scanned: two convert, two compatible, one error.
5. A short H.264 is converted and verified as `hevc` without modifying the original; replacement is tested with a temporary copy.
6. The moved macOS bundle starts with `PATH=/usr/bin:/bin`, scans, converts, and shuts down.
7. `cargo fmt --check`, `cargo check --workspace`, and `git diff --check` pass.
8. README, `docs/`, and this guide are updated if architecture or commands change.
9. The size budget passes; report the measured app and ZIP sizes.
