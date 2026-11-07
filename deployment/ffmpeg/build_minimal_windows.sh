#!/usr/bin/env bash
# Build the minimal FFmpeg used by the Windows application.
#
# Must run inside an MSYS2 MINGW64 shell with:
#   make, mingw-w64-x86_64-toolchain, mingw-w64-x86_64-nasm,
#   mingw-w64-x86_64-pkgconf, mingw-w64-x86_64-x265
#
# Every DLL is collected next to the executables so the application works from
# any folder without MinGW installed.
set -euo pipefail

VERSION="${FFMPEG_VERSION:-9.0.2}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
WORK="${FOTOGRAMMA_FFMPEG_WORK:-$ROOT/deployment/vendor/rust/build}"
PREFIX="${1:-$WORK/ffmpeg-$VERSION}"
SRC="$WORK/ffmpeg-$VERSION"
VENDOR="$ROOT/deployment/vendor/rust/Windows-amd64"
JOBS="$(nproc 2>/dev/null || echo 4)"

mkdir -p "$WORK"
if [ ! -d "$SRC" ]; then
  echo "Downloading FFmpeg $VERSION ..."
  curl -fL --retry 3 -o "$WORK/ffmpeg-$VERSION.tar.xz" \
    "https://ffmpeg.org/releases/ffmpeg-$VERSION.tar.xz"
  tar -xf "$WORK/ffmpeg-$VERSION.tar.xz" -C "$WORK"
fi

cd "$SRC"

./configure \
  --prefix="$PREFIX" \
  --disable-everything \
  --disable-autodetect \
  --disable-network \
  --disable-doc \
  --disable-ffplay \
  --disable-avdevice \
  --enable-shared \
  --disable-static \
  --enable-gpl \
  --enable-libx265 \
  --enable-ffmpeg \
  --enable-ffprobe \
  --enable-decoder=hevc,h264,vp8,vp9,av1,mpeg4,mpeg2video,mpeg1video \
  --enable-decoder=vc1,wmv2,wmv3,prores,dnxhd,dvvideo,mjpeg,theora,h263,h263p \
  --enable-decoder=flv,rawvideo,msmpeg4v3,ffv1,huffyuv \
  --enable-decoder=aac,ac3,eac3,dca,truehd,mlp,opus,vorbis,flac,alac,mp3,mp2 \
  --enable-decoder=pcm_s16le,pcm_s24le,pcm_s32le,pcm_f32le,pcm_s16be,pcm_s24be \
  --enable-decoder=pcm_s32be,pcm_u8,pcm_alaw,pcm_mulaw \
  --enable-decoder=subrip,ass,ssa,webvtt,mov_text,hdmv_pgs_subtitle,dvd_subtitle \
  --enable-decoder=dvb_subtitle,text,eia_608,microdvd,sami \
  --enable-encoder=libx265,aac,mpeg4,wrapped_avframe,pcm_s16le \
  --enable-demuxer=mov,matroska,rawvideo \
  --enable-muxer=mov,mp4,matroska,null \
  --enable-protocol=file,pipe \
  --enable-filter=aformat,anull,aresample,asetpts,atrim,buffer,buffersink \
  --enable-filter=abuffer,abuffersink,copy,format,fps,null,scale,setpts,trim \
  --enable-parser=aac,ac3,av1,dca,flac,h264,hevc,mpegaudio,mpeg4video,mpegvideo \
  --enable-parser=opus,vorbis,vp3,vp8,vp9 \
  --enable-bsf=null

make -j"$JOBS"
make install

rm -rf "$VENDOR"
mkdir -p "$VENDOR/bin"
cp "$PREFIX/bin/ffmpeg.exe" "$PREFIX/bin/ffprobe.exe" "$VENDOR/bin/"
cp "$PREFIX"/bin/*.dll "$VENDOR/bin/" 2>/dev/null || true
export PATH="$VENDOR/bin:$PREFIX/bin:$PATH"

collect_dlls() {
  local binary="$1"
  local path name
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    name="$(basename "$path")"
    [ -f "$VENDOR/bin/$name" ] && continue
    case "$path" in
      /mingw64/*|/ucrt64/*|/clang64/*) ;;
      *) continue ;;
    esac
    cp "$path" "$VENDOR/bin/$name"
    collect_dlls "$VENDOR/bin/$name"
  done < <(ldd "$binary" 2>/dev/null | awk '/=>/ {print $3}' | grep -v '^not$' || true)
}

collect_dlls "$VENDOR/bin/ffmpeg.exe"
collect_dlls "$VENDOR/bin/ffprobe.exe"

"$VENDOR/bin/ffmpeg.exe" -hide_banner -version | head -n 1
"$VENDOR/bin/ffprobe.exe" -hide_banner -version | head -n 1
echo "Vendored toolchain: $VENDOR"
ls -lh "$VENDOR/bin"
