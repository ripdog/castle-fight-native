#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WC3_INSTALL="${WC3_INSTALL:-/mnt/gamessd_linux/Games/Warcraft3}"
CACHE="$ROOT/.wc3-base"
SOURCE="$CACHE/source"
CASC="$ROOT/.local-tools/bin/casc_extract"

if [[ ! -f "$WC3_INSTALL/.build.info" ]]; then
    echo "Warcraft III CASC install not found at: $WC3_INSTALL" >&2
    echo "Set WC3_INSTALL=/path/to/Warcraft3 to override." >&2
    exit 1
fi

"$ROOT/tools/wc3-map/bootstrap-casclib.sh"

rm -rf "$SOURCE"
mkdir -p "$SOURCE/install" "$SOURCE/base" "$SOURCE/custom_v0" "$SOURCE/custom_v1" "$SOURCE/melee_v0" "$SOURCE/enus"
install -m 0644 "$WC3_INSTALL/.build.info" "$SOURCE/install/.build.info"

# Cache every World Editor game-data overlay that W3I can select. The W3I
# enum is Default=0, Custom101=1, MeleeLatestPatch=2. For TFT maps the default
# custom data uses custom_v1; custom_v0 is the legacy 1.01/ROC-compatible set.
# Keeping all three locally lets resolve-base-data.py select from W3I rather
# than baking one balance assumption into the extraction cache.
"$CASC" "$WC3_INSTALL" extract-prefix \
    'war3.w3mod:_balance\custom_v0.w3mod:units\' \
    "$SOURCE/custom_v0/units"
"$CASC" "$WC3_INSTALL" extract-prefix \
    'war3.w3mod:_balance\custom_v1.w3mod:units\' \
    "$SOURCE/custom_v1/units"
"$CASC" "$WC3_INSTALL" extract-prefix \
    'war3.w3mod:_balance\melee_v0.w3mod:units\' \
    "$SOURCE/melee_v0/units"

# Base object/editor data. Limit this prefix to text/data tables; the same
# virtual directory also contains ~180 MiB of presentation assets.
"$CASC" "$WC3_INSTALL" extract-prefix 'war3.w3mod:units\' "$SOURCE/base/units" '.slk'
"$CASC" "$WC3_INSTALL" extract-prefix 'war3.w3mod:units\' "$SOURCE/base/units" '.txt'
"$CASC" "$WC3_INSTALL" extract-prefix 'war3.w3mod:doodads\' "$SOURCE/base/doodads" '.slk'
"$CASC" "$WC3_INSTALL" extract-prefix 'war3.w3mod:doodads\' "$SOURCE/base/doodads" '.txt'

# Authoritative pathing masks used by buildings/destructables/doodads.
"$CASC" "$WC3_INSTALL" extract-prefix 'war3.w3mod:pathtextures\' "$SOURCE/base/pathtextures" '.tga'

# English editor/game labels used to turn raw field/string keys into readable
# names without depending on an installed locale at decode time.
"$CASC" "$WC3_INSTALL" extract-prefix \
    'war3.w3mod:_locales\enus.w3mod:units\' \
    "$SOURCE/enus/units" '.txt'
"$CASC" "$WC3_INSTALL" extract-prefix \
    'war3.w3mod:_locales\enus.w3mod:doodads\' \
    "$SOURCE/enus/doodads" '.txt'
"$CASC" "$WC3_INSTALL" extract \
    'war3.w3mod:ui\worldeditdata.txt' \
    "$SOURCE/base/ui/worldeditdata.txt"
"$CASC" "$WC3_INSTALL" extract \
    'war3.w3mod:_locales\enus.w3mod:ui\worldeditstrings.txt' \
    "$SOURCE/enus/ui/worldeditstrings.txt"
"$CASC" "$WC3_INSTALL" extract \
    'war3.w3mod:_locales\enus.w3mod:ui\worldeditgamestrings.txt' \
    "$SOURCE/enus/ui/worldeditgamestrings.txt"

printf 'cached Warcraft III base data under %s\n' "$SOURCE"
find "$SOURCE" -type f -printf '%P\t%s\n' | sort
