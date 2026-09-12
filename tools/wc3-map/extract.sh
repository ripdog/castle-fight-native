#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
default_map="$repo_root/docs/original_map/5329_Castle_Fight_DE_beta9.27_w3p.w3x"
map_path="${1:-$default_map}"
output_dir="${2:-$repo_root/docs/original_map/extracted}"
work_dir="$repo_root/.map-work/original-map"
raw_dir="$work_dir/raw"
translator_input_dir="$work_dir/translator-input"
translated_dir="$work_dir/translated"
mpq_extractor="$repo_root/.local-tools/bin/mpq_extract"
translator="$repo_root/.local-tools/WC3MapTranslator/dist/src/cli.js"

if [[ ! -f "$map_path" ]]; then
    echo "map not found: $map_path" >&2
    exit 1
fi

if [[ ! -x "$mpq_extractor" || "$repo_root/tools/wc3-map/mpq_extract.cpp" -nt "$mpq_extractor" ]]; then
    "$repo_root/tools/wc3-map/bootstrap-stormlib.sh"
fi
if [[ ! -f "$translator" ]]; then
    "$repo_root/tools/wc3-map/bootstrap-translator.sh"
fi

rm -rf "$work_dir"
mkdir -p "$raw_dir" "$translator_input_dir" "$translated_dir"

"$mpq_extractor" "$map_path" "$raw_dir" > "$work_dir/archive-extract.tsv"

# WC3MapTranslator handles the supported binary object/terrain/doodad formats.
# Feed it only those members: its current W3I parser targets v33 while this map
# is v31, and its WTS decoder recodes this map's UTF-8 strings incorrectly.
for member in war3map.w3e war3map.doo war3map.w3u war3map.w3t war3map.w3b war3map.w3d war3map.w3a war3map.w3h; do
    cp "$raw_dir/$member" "$translator_input_dir/$member"
done
node "$translator" "$translator_input_dir" "$translated_dir" --toJson --force

for required in terrain.json doodads.json obj-units.json obj-items.json obj-abilities.json obj-buffs.json obj-destructables.json obj-doodads.json; do
    if [[ ! -f "$translated_dir/$required" ]]; then
        echo "translator did not produce required output: $required" >&2
        exit 1
    fi
done

rm -rf "$output_dir"
mkdir -p "$output_dir"
cp "$work_dir/archive-extract.tsv" "$output_dir/archive-extract.tsv"

python "$repo_root/tools/wc3-map/decode_map.py" \
    --map "$map_path" \
    --raw "$raw_dir" \
    --translated "$translated_dir" \
    --output "$output_dir"

if [[ -f "$repo_root/.wc3-base/source/install/.build.info" ]]; then
    python "$repo_root/tools/wc3-map/resolve-base-data.py" \
        --base "$repo_root/.wc3-base/source" \
        --map-extracted "$output_dir" \
        --output "$output_dir/resolved"
else
    echo "base-data cache not found; run tools/wc3-map/extract-base-data.sh to enable inherited object resolution" >&2
fi

echo "decoded original map to $output_dir"
