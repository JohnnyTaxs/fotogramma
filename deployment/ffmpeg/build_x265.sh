#!/usr/bin/env bash
# Build a slim x265 for the packaged application (Linux and Windows).
#
# Distribution packages ship x265 with all three bit depths, which makes the
# combined library 16 MB on Ubuntu and 22 MB on MSYS2. The application only
# offers 8-bit and 10-bit output, so this script builds exactly those two,
# combines them into one shared library the way x265's own multilib build
# does, and leaves out the 12-bit and HDR10+ code.
#
# Usage:
#   deployment/ffmpeg/build_x265.sh [prefix]
#
# Must run on the target system (or inside MSYS2 UCRT64 on Windows) with
# cmake, a C++ compiler, nasm, and curl. On Windows it also needs git: x265's
# Version.cmake reads x265Version.txt through Git, and without it the release
# tag stays empty and the resource-compiler configuration fails.
set -euo pipefail

VERSION="${X265_VERSION:-4.3}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
WORK="${FOTOGRAMMA_FFMPEG_WORK:-$ROOT/deployment/vendor/rust/build}"
PREFIX="${1:-$WORK/x265-$VERSION}"
SRC="$WORK/x265_$VERSION"
BUILD="$WORK/x265-$VERSION-build"
JOBS="$(getconf _NPROCESSORS_ONLN 2>/dev/null || nproc 2>/dev/null || echo 4)"
COMMON=(
  -DCMAKE_BUILD_TYPE=Release
  -DENABLE_CLI=OFF
  -DENABLE_HDR10_PLUS=OFF
  -DENABLE_LIBNUMA=OFF
  -DENABLE_ASSEMBLY=ON
)

# The packaged toolchain must not carry a separate C++ runtime: linking it
# into the shared x265 library costs less than shipping libstdc++ (3 MB on
# Linux, 2.6 MB on Windows) next to it.
SHARED_LINK=()
case "$(uname -s)" in
  Linux | MINGW* | MSYS*)
    SHARED_LINK+=("-DCMAKE_SHARED_LINKER_FLAGS=-static-libstdc++ -static-libgcc")
    ;;
esac

mkdir -p "$WORK"
if [ ! -d "$SRC" ]; then
  echo "Downloading x265 $VERSION ..."
  curl -fL --retry 5 --retry-all-errors -o "$WORK/x265-$VERSION.tar.gz" \
    "https://github.com/Multicorewareinc/x265/releases/download/$VERSION/x265_$VERSION.tar.gz"
  tar -xf "$WORK/x265-$VERSION.tar.gz" -C "$WORK"
fi

rm -rf "$BUILD"
mkdir -p "$BUILD/8bit"

# 10-bit objects: no public API, no shared library, no CLI.
cmake -S "$SRC/source" -B "$BUILD/10bit" \
  "${COMMON[@]}" \
  -DHIGH_BIT_DEPTH=ON \
  -DEXPORT_C_API=OFF \
  -DENABLE_SHARED=OFF
cmake --build "$BUILD/10bit" --parallel "$JOBS"

# The 8-bit build exposes the API and links the 10-bit objects into it, so
# FFmpeg sees one library that handles both bit depths.
cp "$BUILD/10bit/libx265.a" "$BUILD/8bit/libx265_main10.a"
cd "$BUILD/8bit"
cmake -S "$SRC/source" -B "$BUILD/8bit" \
  "${COMMON[@]}" \
  ${SHARED_LINK[@]+"${SHARED_LINK[@]}"} \
  -DENABLE_SHARED=ON \
  -DLINKED_10BIT=ON \
  -DEXTRA_LINK_FLAGS=-L. \
  -DEXTRA_LIB=x265_main10.a \
  -DCMAKE_INSTALL_PREFIX="$PREFIX"
cmake --build "$BUILD/8bit" --parallel "$JOBS"
cmake --install "$BUILD/8bit"

echo "Slim x265 installed in $PREFIX"
find "$PREFIX" -maxdepth 2 -name "*x265*" -exec ls -lh {} +
