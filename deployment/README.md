# Native build and distribution

## Requirements

- A stable Rust toolchain (`cargo`, `rustc`);
- Python 3.10+ for the packaging and collector scripts;
- on macOS: Xcode command line tools, `pkgconf`, and `x265` from Homebrew;
- on Linux: `build-essential`, `pkg-config`, `nasm`, `cmake`, `patchelf`, plus the X11/Wayland/OpenGL development packages listed in the CI workflow;
- on Windows: MSYS2 UCRT64 with `mingw-w64-ucrt-x86_64-toolchain`, `nasm`, `cmake`, `pkgconf`, and the MSVC Rust toolchain on the host.

None of these are required on the destination computer.

## Minimal FFmpeg

FFmpeg is no longer downloaded as a large community archive. It is built from the 9.0.2 source with only what the app uses:

```bash
# macOS and Linux
deployment/ffmpeg/build_minimal.sh

# Windows, inside an MSYS2 UCRT64 shell
deployment/ffmpeg/build_minimal_windows.sh
```

Enabled components:

- demuxers `mov`, `matroska`, `rawvideo`;
- muxers `mov`, `mp4`, `matroska`, `null`;
- protocols `file` and `pipe`;
- encoders `libx265`, `aac`, `mpeg4` (self-test only), `wrapped_avframe`, `pcm_s16le`;
- decoders for the conversion inputs, copied audio and subtitle tracks, and HEVC output (H.264, VP8/VP9, AV1, MPEG-1/2/4, VC-1, ProRes, DNxHD, DV, MJPEG, Theora, H.263, FLV, FFV1, HuffYUV, AAC, AC-3/E-AC-3, DTS, TrueHD, Opus, Vorbis, FLAC, ALAC, MP2/MP3, PCM, SubRip, ASS/SSA, WebVTT, mov_text, PGS, DVD/DVB subtitles);
- filters needed by the conversion graph and full-decode verification (`scale`, `format`, `aresample`, `aformat`, `null`, `anull`, and buffers).

Network support, devices, x264, SVT-AV1, libvpx, OpenSSL, VMAF, mp3lame, and opus encoders are excluded. The resulting toolchain is about 17 MB instead of 38 MB on macOS.

### x265

macOS uses Homebrew's arm64 x265 (7 MB). On Linux and Windows `build_x265.sh` builds the encoder from source instead of taking the distribution package: those packages carry 8, 10 and 12-bit support, and the combined library is 17 MB on Ubuntu and 22 MB on MSYS2. The script builds only the two bit depths the presets use, links them into one shared library the way x265's own multilib build does, and statically links the C++ runtime so no `libstdc++` has to ship next to it. The result is about 12 MB on Linux.

### Collected layout

```
deployment/vendor/rust/<System>-<arch>/
  bin/ffmpeg
  bin/ffprobe
  lib/*.dylib | *.so.N | *.dll
  manifest.json            # macOS and Linux
```

`collect_macos.py` copies non-system dylibs, rewrites install names to `@loader_path`, ad-hoc signs every Mach-O file, and verifies that both binaries start.

`collect_linux.py` copies the FFmpeg sonames and non-glibc dependencies, sets an `$ORIGIN/../lib` runpath with `patchelf`, strips the ELF symbol tables, and verifies both binaries start. glibc libraries (`libc`, `libm`, `libmvec`, the loader) are never copied: they must match the host.

`build_minimal_windows.sh` copies every non-system DLL next to the executables because Windows resolves them from the executable folder, then strips the PE files. The ELF and PE installs keep large symbol tables that would otherwise break the size budget.

`deployment/vendor/` is ignored by Git. Rebuild it on each platform; do not commit binaries.

## Build the application

```bash
python3 deployment/build_rust.py
```

The script:

1. reads the version from `Cargo.toml` (or `FOTOGRAMMA_VERSION`);
2. runs `cargo build --release --locked -p fotogramma -p fotogramma-core` (the app and the command line tool);
3. assembles the platform bundle;
4. creates `dist/Fotogramma-<System>-<arch>.zip`;
   on macOS it also creates `dist/Fotogramma-Darwin-arm64.dmg` (see below);
5. on macOS, extracts the ZIP again and runs `codesign --verify --deep --strict` on both copies;
6. runs the packaged `fotogramma-cli --version` and an empty scan with a clean `PATH`, to prove it finds the bundled `ffprobe`;
7. fails if the app exceeds 45 MB or the ZIP exceeds 18 MB.

Set `FOTOGRAMMA_RUST_SKIP_BUILD=1` to reuse an existing release binary.

### macOS disk image

The DMG shows the app and an *Applications* shortcut over a branded background with an arrow. It is built with [`dmgbuild`](https://pypi.org/project/dmgbuild/), which writes the window layout (`.DS_Store`) directly: no Finder or AppleScript session is needed, so it works on a headless CI runner.

```bash
python3 -m venv .venv && .venv/bin/pip install dmgbuild
FOTOGRAMMA_REQUIRE_DMG=1 .venv/bin/python deployment/build_rust.py
```

Without `dmgbuild` the script skips the DMG with a warning, unless `FOTOGRAMMA_REQUIRE_DMG=1` (CI). The script mounts the image and checks the app, the `/Applications` symlink, the layout file and the code signature. `deployment/dmg/background.png` and `background@2x.png` come from `deployment/dmg/make_background.py` (needs Pillow); the icon slots in that script and in `build_rust.py` must stay in step. The ZIP is still published because the in-app updater installs it.

### Layouts

- **macOS:** `Fotogramma.app/Contents/MacOS/{fotogramma,fotogramma-cli}`, `Contents/Resources/bin/{ffmpeg,ffprobe}`, `Contents/Resources/lib/*.dylib`, `Contents/Info.plist`, `Contents/Resources/Fotogramma.icns`.
- **Linux:** `Fotogramma/{fotogramma,fotogramma-cli}`, `Fotogramma/bin/{ffmpeg,ffprobe}`, `Fotogramma/lib/*.so.N`.
- **Windows:** `Fotogramma/{fotogramma.exe,fotogramma-cli.exe}`, `Fotogramma/bin/{ffmpeg.exe,ffprobe.exe}`, `Fotogramma/bin/*.dll`.

The application searches the executable folder, `bin/` beside it, and the macOS `Resources/bin` folder before falling back to `PATH`.

## Verification

```bash
# Start the GUI
Fotogramma.app/Contents/MacOS/fotogramma \
  --directory /path/to/fixtures --scan-on-start --smoke-test
```

Before release:

1. Extract the ZIP in a new directory.
2. Start with `PATH=/usr/bin:/bin`.
3. Verify startup and a complete scan.
4. Confirm that the scan uses the bundled tools (`FOTOGRAMMA_FFMPEG_DIR` is not required in the bundle).
5. Convert a short H.264 clip with the bundled `ffmpeg` and confirm `hevc` with the bundled `ffprobe`.
6. Run the pipeline test with `FOTOGRAMMA_FFMPEG_DIR` pointing at the packaged `bin/`; it verifies copy mode and atomic replacement.
7. Simulate a failed final check to confirm the original is preserved.

The macOS ZIP must be created with `ditto -c -k --sequesterRsrc --keepParent`. A generic ZIP writer can turn framework symbolic links into copies and invalidate the ad-hoc signature, after which macOS kills the app at launch. `build_rust.py` does this and verifies the extracted bundle before publishing.

## Signing

The macOS build is ad-hoc signed but not notarized. On macOS 15 and later, allow the first launch in **System Settings → Privacy & Security → Open Anyway**; on older versions, use right-click → Open. Public distribution without warnings requires a Developer ID certificate and Apple notarization. Windows SmartScreen may also require a code-signing strategy.

## Licensing

The bundled FFmpeg is configured with `--enable-gpl --enable-libx265`. The FFmpeg source, build flags, and x265 dependency are published here; the application source remains MIT.

## Icons

`deployment/make_icons.py` (needs Pillow) rebuilds `fotogramma.iconset/`, `Fotogramma.icns` (macOS only, through `iconutil`), `Fotogramma.ico` and `Fotogramma-window.png` from `Fotogramma.png`. The macOS icon has the usual margin and shadow; the Windows icon and the logo inside the app fill the canvas. Do not edit the PNGs by hand.

`deployment/make_mascot.py` (needs Pillow) draws `mascot.png` (6 run frames and 2 cheer frames) and `tv.png` (off, on) for the easter egg on the progress bar; `--variant` picks the girl and `--preview` writes a magnified sheet. The art is original and belongs to the project.
