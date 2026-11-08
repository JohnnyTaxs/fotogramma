#!/usr/bin/env bash
# Build the minimal FFmpeg used by the packaged application (macOS and Linux).
#
# Only the containers, codecs and filters the application actually needs are
# enabled: MP4/MKV demuxing, HEVC encoding through libx265, every audio and
# subtitle format that may be copied, full-decode verification, and the raw
# video input used by the self-test fixture generator.
#
# Usage:
#   deployment/ffmpeg/build_minimal.sh [prefix]
#
# The default prefix is deployment/vendor/rust/build/ffmpeg-<version>. After a
# successful install the matching vendor directory is refreshed.
set -euo pipefail

VERSION="${FFMPEG_VERSION:-9.0.2}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
WORK="${FOTOGRAMMA_FFMPEG_WORK:-$ROOT/deployment/vendor/rust/build}"
PREFIX="${1:-$WORK/ffmpeg-$VERSION}"
SRC="$WORK/ffmpeg-$VERSION"
JOBS="$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)"

mkdir -p "$WORK"
if [ ! -d "$SRC" ]; then
  echo "Downloading FFmpeg $VERSION ..."
  curl -fL --retry 3 -o "$WORK/ffmpeg-$VERSION.tar.xz" \
    "https://ffmpeg.org/releases/ffmpeg-$VERSION.tar.xz"
  tar -xf "$WORK/ffmpeg-$VERSION.tar.xz" -C "$WORK"
fi

X265_VERSION="${X265_VERSION:-4.3}"
X265_PREFIX=""

case "$(uname -s)" in
  Darwin)
    # Homebrew's x265 is a 7 MB arm64 build; rebuilding it would not save
    # anything, so the packaged macOS toolchain keeps using it.
    ;;
  Linux)
    # Distributions ship x265 with 8, 10 and 12-bit support (16 MB). Build a
    # slim 8+10-bit library instead: it is the only bit-depth pair the
    # application offers, and FFmpeg links it through pkg-config.
    X265_PREFIX="$WORK/x265-$X265_VERSION"
    if [ ! -f "$X265_PREFIX/lib/pkgconfig/x265.pc" ]; then
      "$HERE/build_x265.sh" "$X265_PREFIX"
    fi
    export PKG_CONFIG_PATH="$X265_PREFIX/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
    export FOTOGRAMMA_EXTRA_LIBDIRS="$X265_PREFIX/lib${FOTOGRAMMA_EXTRA_LIBDIRS:+:$FOTOGRAMMA_EXTRA_LIBDIRS}"
    ;;
esac

cd "$SRC"

COMMON=(
  --prefix="$PREFIX"
  --disable-everything
  --disable-autodetect
  --disable-network
  --disable-doc
  --disable-ffplay
  --disable-avdevice
  --enable-shared
  --disable-static
  --enable-gpl
  --enable-libx265
  --enable-ffmpeg
  --enable-ffprobe
  # Decoders needed to read every conversion input, copied audio, and the
  # HEVC output during the full-decode verification.
  --enable-decoder=hevc,h264,vp8,vp9,av1,mpeg4,mpeg2video,mpeg1video
  --enable-decoder=vc1,wmv2,wmv3,prores,dnxhd,dvvideo,mjpeg,theora,h263,h263p
  --enable-decoder=flv,rawvideo,msmpeg4v3,ffv1,huffyuv
  --enable-decoder=aac,ac3,eac3,dca,truehd,mlp,opus,vorbis,flac,alac,mp3,mp2
  --enable-decoder=pcm_s16le,pcm_s24le,pcm_s32le,pcm_f32le,pcm_s16be,pcm_s24be
  --enable-decoder=pcm_s32be,pcm_u8,pcm_alaw,pcm_mulaw
  --enable-decoder=subrip,ass,ssa,webvtt,mov_text,hdmv_pgs_subtitle,dvd_subtitle
  --enable-decoder=dvb_subtitle,text,eia_608,microdvd,sami
  # HEVC output, optional AAC audio, and the tiny mpeg4 encoder used only to
  # generate the self-test source with the packaged toolchain.
  --enable-encoder=libx265,aac,mpeg4,wrapped_avframe,pcm_s16le
  --enable-demuxer=mov,matroska,rawvideo
  --enable-muxer=mov,mp4,matroska,null
  --enable-protocol=file,pipe
  --enable-filter=aformat,anull,aresample,asetpts,atrim,buffer,buffersink
  --enable-filter=abuffer,abuffersink,copy,format,fps,null,scale,setpts,trim
  --enable-parser=aac,ac3,av1,dca,flac,h264,hevc,mpegaudio,mpeg4video,mpegvideo
  --enable-parser=opus,vorbis,vp3,vp8,vp9
  --enable-bsf=null
)

case "$(uname -s)" in
  Darwin)
    COMMON+=(--disable-x86asm)
    ;;
  Linux)
    # The collector rewrites the runpath of every copied object with
    # patchelf, so the build itself does not need a hard-coded prefix.
    ;;
  *)
    echo "Unsupported system: $(uname -s)" >&2
    exit 1
    ;;
esac

./configure "${COMMON[@]}"
make -j"$JOBS"
make install

SYSTEM="$(uname -s)"
ARCH="$(python3 -c 'import platform; print(platform.machine().lower())')"
VENDOR="$ROOT/deployment/vendor/rust/$SYSTEM-$ARCH"

case "$SYSTEM" in
  Darwin)
    python3 "$HERE/collect_macos.py" "$PREFIX" "$VENDOR"
    ;;
  Linux)
    python3 "$HERE/collect_linux.py" "$PREFIX" "$VENDOR"
    ;;
esac

echo "Minimal FFmpeg installed in $PREFIX"
echo "Vendored toolchain: $VENDOR"
