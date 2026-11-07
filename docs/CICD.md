# CI/CD with GitHub Actions

The workflow `.github/workflows/build-release.yml` runs on every push to `main` or `rust-rewrite`, pull request, `v*` tag, and manual dispatch.

## Platforms

| Job | Runner | Output |
| --- | --- | --- |
| Linux | `ubuntu-latest` | `Fotogramma-Linux-x86_64.zip` |
| Windows | `windows-latest` (MSYS2 MINGW64 for FFmpeg) | `Fotogramma-Windows-amd64.zip` |
| macOS ARM | `macos-15` | `Fotogramma-Darwin-arm64.zip` |

**macOS Intel is intentionally excluded.** Architecture names come from `platform.machine()` during the build, so archive names change only if a runner changes. Releases are never cross-compiled: each job builds and packages on its own operating system.

## Steps per build job

1. `actions/checkout@v4`.
2. `dtolnay/rust-toolchain@stable`.
3. `Swatinem/rust-cache@v2` for the workspace.
4. Platform build dependencies:
   - Linux: `build-essential`, `pkg-config`, `nasm`, `patchelf`, `libx265-dev`, OpenGL, Mesa drivers (for the headless wgpu snapshots), X11, xkbcommon, and Wayland development packages.
   - macOS ARM: `x265` and `pkgconf` through Homebrew.
   - Windows: MSYS2 with `mingw-w64-x86_64-toolchain`, `nasm`, `pkgconf`, and `x265`.
5. `deployment/ffmpeg/build_minimal.sh` (or `build_minimal_windows.sh`) downloads FFmpeg 9.0.2, configures the minimal component list, builds it, and collects the relocatable toolchain into `deployment/vendor/rust/<System>-<arch>/`.
6. `cargo test --workspace --locked`.
7. The two real-pipeline tests again with `FOTOGRAMMA_FFMPEG_DIR` pointing at the freshly built tools.
8. `deployment/build_rust.py`: release build, bundle, ad-hoc signature (macOS), ZIP, round-trip verification, and the 35 MB / 18 MB size budget.
9. Linux and macOS also generate a tiny clip with the bundled FFmpeg and scan it with a clean `PATH` to prove the packaged tools are used.
10. `actions/upload-artifact@v4` with a 3-day retention.

## Release

On a `v*` tag the `release` job downloads every ZIP and calls `deployment/create_release.py`. The script:

- uses `GITHUB_TOKEN` with `GITHUB_API_URL` and `GITHUB_REPOSITORY`;
- creates the release with the notes in `docs/releases/<tag>.md` when present;
- deletes an asset with the same name before uploading, so re-running a tag does not duplicate files.

Never print `GITHUB_TOKEN` or `FORGEJO_TOKEN`.

## Local equivalents

```bash
# Tests
cd rust && cargo test --workspace --locked

# Minimal FFmpeg + package (macOS/Linux)
deployment/ffmpeg/build_minimal.sh
python3 deployment/build_rust.py

# Release upload
python3 deployment/create_release.py path/to/zips
```
