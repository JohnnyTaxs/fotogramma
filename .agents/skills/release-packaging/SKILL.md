---
name: release-packaging
description: Fotogramma's packaging, minimal FFmpeg build, CI workflow and release rules. Load before touching deployment/, .github/workflows/, the size budget, code signing, or publishing a release.
---

# Packaging, CI and releases

The invariants in `AGENTS.md` still apply (onedir, size budget, verified updates). These are the details behind them.

## Minimal FFmpeg

Windows builds use `deployment/ffmpeg/build_minimal_windows.sh` inside MSYS2 MINGW64. Never substitute the full Homebrew/community FFmpeg archive: it breaks the size budget.

Linux and Windows build x265 with `deployment/ffmpeg/build_x265.sh` (8-bit + 10-bit only, static C++ runtime) instead of installing the distribution package; macOS uses Homebrew's arm64 x265.

## Build and assets

- `deployment/ffmpeg/build_minimal.sh` and `build_minimal_windows.sh` define the only supported FFmpeg component list. Adding a codec or filter must be justified by a real app path and re-measured against the budget.
- `deployment/ffmpeg/collect_macos.py` rewrites install names to `@loader_path`, re-signs every Mach-O file, and verifies relocation.
- `deployment/ffmpeg/collect_linux.py` sets an `$ORIGIN/../lib` runpath with `patchelf`.
- macOS archives use `ditto -c -k --sequesterRsrc --keepParent`; `deployment/build_rust.py` extracts the ZIP again and runs `codesign --verify --deep --strict` before publishing.
- The macOS `Info.plist` embeds the version from `Cargo.toml`.
- Windows embeds the icon and version through `rust/crates/app/build.rs` (`winresource`).

## CI/CD and releases

`.github/workflows/build-release.yml` runs on Linux, Windows, and macOS ARM:

- builds the minimal FFmpeg on each platform;
- runs `cargo test --workspace --locked`;
- reruns the real pipeline tests against the freshly built tools;
- packages with `deployment/build_rust.py` and enforces the size budget;
- on a `v*` tag, publishes through `deployment/create_release.py`.

The minimal FFmpeg of Linux and Windows is cached on the hash of `deployment/ffmpeg/**`: change a script there and the next run rebuilds it. Third-party actions are pinned to a commit SHA (Dependabot updates them) and the workflow token is read-only except in the release job. Do not add macOS Intel or Forgejo-specific runner configuration. Never print `GITHUB_TOKEN` or `FORGEJO_TOKEN`. Release tags follow SemVer (`v2.0.0`).
