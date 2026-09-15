#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
version="${1:?usage: tools/wc3-map/extract-release.sh <map-version> <revision>}"
revision="${2:?usage: tools/wc3-map/extract-release.sh <map-version> <revision>}"
resolver="$repo_root/tools/wc3-map/release_manifest.py"

source_rel="$(python "$resolver" path source "$version" "$revision")"
output_rel="$(python "$resolver" path extraction "$version" "$revision" --require-pending)"

python "$resolver" verify "$version" "$revision"

exec "$repo_root/tools/wc3-map/extract.sh" \
    "$repo_root/$source_rel" \
    "$repo_root/$output_rel"
