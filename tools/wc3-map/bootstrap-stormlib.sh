#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
source_dir="$repo_root/.local-tools/StormLib"
build_dir="$source_dir/build"
bin_dir="$repo_root/.local-tools/bin"
stormlib_repo="https://github.com/ladislav-zezula/StormLib.git"
stormlib_commit="44ebfbfc109d76e2a85bbd5d8b0c949df7e65c6f"

mkdir -p "$repo_root/.local-tools" "$bin_dir"

if [[ ! -d "$source_dir/.git" ]]; then
    git clone "$stormlib_repo" "$source_dir"
fi

git -C "$source_dir" fetch --depth 1 origin "$stormlib_commit"
git -C "$source_dir" checkout --detach "$stormlib_commit"

license_line="$(head -n 1 "$source_dir/LICENSE" | tr -d '\r')"
if [[ "$license_line" != "The MIT License (MIT)" ]]; then
    echo "unexpected StormLib license header: $license_line" >&2
    exit 1
fi

cmake -S "$source_dir" -B "$build_dir" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DSTORM_USE_BUNDLED_LIBRARIES=ON \
    -DSTORM_BUILD_TESTS=OFF
cmake --build "$build_dir"

c++ -std=c++17 -O2 \
    -I"$source_dir/src" \
    "$repo_root/tools/wc3-map/mpq_extract.cpp" \
    "$build_dir/libstorm.a" \
    -pthread \
    -o "$bin_dir/mpq_extract"

echo "built $bin_dir/mpq_extract from StormLib $stormlib_commit (MIT)"
