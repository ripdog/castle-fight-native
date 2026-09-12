#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$ROOT/.local-tools/CascLib"
BUILD="$SRC/build"
BIN="$ROOT/.local-tools/bin/casc_extract"
REVISION="2a280f5a231966dc5d1b534978dd9f9f04a374cd"
REPO="https://github.com/ladislav-zezula/CascLib.git"

mkdir -p "$ROOT/.local-tools/bin"

if [[ ! -d "$SRC/.git" ]]; then
    git clone "$REPO" "$SRC"
fi

git -C "$SRC" fetch --depth 1 origin "$REVISION"
git -C "$SRC" checkout --detach "$REVISION"

grep -q 'The MIT License' "$SRC/LICENSE" || {
    echo "CascLib license was not the expected MIT license" >&2
    exit 1
}

cmake -S "$SRC" -B "$BUILD" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
    -DCASC_BUILD_TESTS=OFF
cmake --build "$BUILD" -j "$(nproc)"

g++ -std=c++17 -O2 -Wall -Wextra -pedantic \
    -I "$SRC/src" \
    "$ROOT/tools/wc3-map/casc_extract.cpp" \
    -L "$BUILD" -Wl,-rpath,"$BUILD" -lcasc \
    -o "$BIN"

echo "built $BIN from CascLib $REVISION (MIT)"
