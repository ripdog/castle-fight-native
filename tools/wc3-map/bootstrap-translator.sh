#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
source_dir="$repo_root/.local-tools/WC3MapTranslator"
translator_repo="https://github.com/ChiefOfGxBxL/WC3MapTranslator.git"
translator_commit="7d477ebb5cea445bee7915fe72cd93ee399252b7"

mkdir -p "$repo_root/.local-tools"

if [[ ! -d "$source_dir/.git" ]]; then
    git clone "$translator_repo" "$source_dir"
fi

git -C "$source_dir" fetch --depth 1 origin "$translator_commit"
git -C "$source_dir" checkout --detach "$translator_commit"

license_line="$(head -n 1 "$source_dir/LICENSE.md" | tr -d '\r')"
if [[ "$license_line" != "# MIT License" ]]; then
    echo "unexpected WC3MapTranslator license header: $license_line" >&2
    exit 1
fi

(
    cd "$source_dir"
    npm ci
    npm run build
)

echo "built WC3MapTranslator $translator_commit (MIT)"
