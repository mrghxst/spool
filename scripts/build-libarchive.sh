#!/usr/bin/env bash
# Builds libarchive (with liblzma, zlib and bzip2) to WebAssembly for the
# post worker's extraction step. The output is committed to
# apps/web/src/lib/archive/vendor/, so normal builds and CI don't need
# Emscripten. Rerun this to update or audit the binary.
#
# Needs: Emscripten (emcc, emcmake, embuilder on PATH), cmake, curl.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)

LIBARCHIVE=3.8.9
XZ=5.8.4
B=${BUILD_DIR:-${TMPDIR:-/tmp}/spool-libarchive}
PREFIX="$B/prefix"
OUT="$ROOT/apps/web/src/lib/archive/vendor"
mkdir -p "$B" "$PREFIX" "$OUT"

fetch() {
  local url=$1 file=$2
  [[ -f "$B/$file" ]] || curl -fsSL "$url" -o "$B/$file"
}
fetch "https://github.com/libarchive/libarchive/releases/download/v$LIBARCHIVE/libarchive-$LIBARCHIVE.tar.xz" "libarchive.tar.xz"
fetch "https://github.com/tukaani-project/xz/releases/download/v$XZ/xz-$XZ.tar.xz" "xz.tar.xz"
(cd "$B" && sha256sum -c - <<SUMS
888c934f9d95648ecb9163dc8e23ab80a476ecb81a8f1154704a227b5b676dde  libarchive.tar.xz
4ce24038fd4221e0d13bc1a2de7a4db56e90b92b3bf75321f6c14be73f65de4b  xz.tar.xz
SUMS
)

rm -rf "$B/libarchive-$LIBARCHIVE" "$B/xz-$XZ"
tar xf "$B/libarchive.tar.xz" -C "$B"
tar xf "$B/xz.tar.xz" -C "$B"

# zlib and bzip2 come from Emscripten's ports.
embuilder build zlib bzip2 >/dev/null
SYSROOT=$(em-config CACHE)/sysroot
LIBDIR="$SYSROOT/lib/wasm32-emscripten"

export CFLAGS="-O3"

# liblzma (decoders are what matter; encoders stay in, they're small).
emcmake cmake -S "$B/xz-$XZ" -B "$B/build-xz" -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$PREFIX" -DBUILD_SHARED_LIBS=OFF \
  -DXZ_TOOL_XZ=OFF -DXZ_TOOL_XZDEC=OFF -DXZ_TOOL_LZMADEC=OFF -DXZ_TOOL_LZMAINFO=OFF \
  -DXZ_TOOL_SCRIPTS=OFF -DXZ_DOC=OFF -DXZ_NLS=OFF -DXZ_THREADS=no -DXZ_SANDBOX=no >/dev/null
cmake --build "$B/build-xz" -j"$(nproc)" >/dev/null
cmake --install "$B/build-xz" >/dev/null

# libarchive: read side only, no crypto (encrypted archives are reported).
emcmake cmake -S "$B/libarchive-$LIBARCHIVE" -B "$B/build-la" -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$PREFIX" -DBUILD_SHARED_LIBS=OFF \
  -DENABLE_TAR=OFF -DENABLE_CPIO=OFF -DENABLE_CAT=OFF -DENABLE_UNZIP=OFF -DENABLE_TEST=OFF \
  -DENABLE_ACL=OFF -DENABLE_XATTR=OFF -DENABLE_ICONV=OFF -DENABLE_LIBXML2=OFF -DENABLE_EXPAT=OFF \
  -DENABLE_OPENSSL=OFF -DENABLE_MBEDTLS=OFF -DENABLE_NETTLE=OFF -DENABLE_CNG=OFF -DENABLE_LIBB2=OFF \
  -DENABLE_LZ4=OFF -DENABLE_ZSTD=OFF -DENABLE_LZO=OFF -DENABLE_PCREPOSIX=OFF -DENABLE_PCRE2POSIX=OFF \
  -DENABLE_LZMA=ON -DLIBLZMA_INCLUDE_DIR="$PREFIX/include" -DLIBLZMA_LIBRARY="$PREFIX/lib/liblzma.a" \
  -DENABLE_ZLIB=ON -DZLIB_INCLUDE_DIR="$SYSROOT/include" -DZLIB_LIBRARY="$LIBDIR/libz.a" \
  -DENABLE_BZip2=ON -DBZIP2_INCLUDE_DIR="$SYSROOT/include" -DBZIP2_LIBRARIES="$LIBDIR/libbz2.a" \
  -DHAVE_LIBLZMA=1 >/dev/null
cmake --build "$B/build-la" -j"$(nproc)" --target archive_static >/dev/null
cmake --install "$B/build-la" >/dev/null 2>&1 || true
LA_LIB=$(find "$B/build-la" -name 'libarchive*.a' | head -n1)

EXPORTS='_malloc,_free,_spool_open,_spool_close,_spool_next,_spool_entry_path,_spool_entry_size,_spool_entry_type,_spool_entry_encrypted,_spool_read,_spool_skip,_spool_error,_spool_format'
emcc -O3 "$ROOT/tools/archive-wasm/spool_archive.c" \
  -I"$B/libarchive-$LIBARCHIVE/libarchive" \
  "$LA_LIB" "$PREFIX/lib/liblzma.a" "$LIBDIR/libz.a" "$LIBDIR/libbz2.a" \
  -lworkerfs.js \
  -sMODULARIZE=1 -sEXPORT_ES6=1 -sEXPORT_NAME=createArchiveModule \
  -sENVIRONMENT=worker -sFILESYSTEM=1 -sALLOW_MEMORY_GROWTH=1 -sINITIAL_MEMORY=33554432 \
  -sSTACK_SIZE=2097152 -sDYNAMIC_EXECUTION=0 -sINCOMING_MODULE_JS_API=locateFile,print,printErr \
  -sEXPORTED_FUNCTIONS="$EXPORTS" \
  -sEXPORTED_RUNTIME_METHODS=FS,HEAPU8,UTF8ToString,stringToNewUTF8 \
  -o "$OUT/archive.mjs"

cat > "$OUT/README.md" <<EOF
# libarchive for Spool (generated)

Built by \`scripts/build-libarchive.sh\` with Emscripten $(emcc --version | head -n1 | sed 's/.*) //'):

- libarchive $LIBARCHIVE (BSD 2-clause)
- xz/liblzma $XZ (0BSD / public domain)
- zlib and bzip2 from Emscripten ports (zlib license, bzip2 license)

\`spool_archive.c\` lives in \`tools/archive-wasm/\`. Licenses are in \`LICENSES.md\`.
EOF
# Third-party licenses that ship with the binary.
{
  echo "# Third-party licenses"
  echo
  echo "## libarchive $LIBARCHIVE"
  echo '```'
  cat "$B/libarchive-$LIBARCHIVE/COPYING"
  echo '```'
  echo
  echo "## xz / liblzma $XZ"
  echo '```'
  cat "$B/xz-$XZ/COPYING.0BSD"
  echo '```'
  echo
  echo "## zlib"
  echo '```'
  cat "$(find "$(em-config CACHE)/ports/zlib" -maxdepth 2 -name LICENSE | head -n1)"
  echo '```'
  echo
  echo "## bzip2"
  echo '```'
  cat "$(find "$(em-config CACHE)/ports/bzip2" -maxdepth 2 -name LICENSE | head -n1)"
  echo '```'
} > "$OUT/LICENSES.md"
ls -l "$OUT"
