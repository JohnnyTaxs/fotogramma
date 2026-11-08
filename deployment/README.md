# Native build and distribution

The directory is named `deployment/` rather than `packaging/` to avoid a conflict with the Python `packaging` package that the 1.x build still uses.

## Requirements

- A stable Rust toolchain (`cargo`, `rustc`);
- Python 3.10+ for the packaging and collector scripts;
- on macOS: Xcode command line tools, `pkgconf`, and `x265` from Homebrew;
- on Linux: `build-essential`, `pkg-config`, `nasm`, `cmake`, `patchelf`, plus the X11/Wayland/OpenGL development packages listed in the CI workflow;
- on Windows: MSYS2 MINGW64 with `mingw-w64-x86_64-toolchain`, `nasm`, `cmake`, `pkgconf`, and the MSVC Rust toolchain on the host.

None of these are required on the destination computer.

## Minimal FFmpeg

FFmpeg is no longer downloaded as a large community archive. It is built from the 9.0.2 source with only what the app uses:

```bash
# macOS and Linux
deployment/ffmpeg/build_minimal.sh

# Windows, inside an MSYS2 MINGW64 shell
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

1. reads the version from `rust/Cargo.toml` (or `FOTOGRAMMA_VERSION`);
2. runs `cargo build --release --locked -p fotogramma`;
3. assembles the platform bundle;
4. creates `dist/Fotogramma-<System>-<arch>.zip`;
5. on macOS, extracts the ZIP again and runs `codesign --verify --deep --strict` on both copies;
6. fails if the app exceeds 35 MB or the ZIP exceeds 18 MB.

Set `FOTOGRAMMA_RUST_SKIP_BUILD=1` to reuse an existing release binary.

### Layouts

- **macOS:** `Fotogramma.app/Contents/MacOS/fotogramma`, `Contents/Resources/bin/{ffmpeg,ffprobe}`, `Contents/Resources/lib/*.dylib`, `Contents/Info.plist`, `Contents/Resources/Fotogramma.icns`.
- **Linux:** `Fotogramma/fotogramma`, `Fotogramma/bin/{ffmpeg,ffprobe}`, `Fotogramma/lib/*.so.N`.
- **Windows:** `Fotogramma/fotogramma.exe`, `Fotogramma/bin/{ffmpeg.exe,ffprobe.exe}`, `Fotogramma/bin/*.dll`.

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
