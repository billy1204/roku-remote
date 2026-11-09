#!/usr/bin/env bash
# Regenerate THIRD-PARTY-LICENSES.md: the notices of every crate in the
# Linux and Windows builds (via cargo-about), then the bundled fonts'.
set -euo pipefail
cd "$(dirname "$0")/.."
command -v cargo-about >/dev/null || { echo "Install it first: cargo install cargo-about --locked"; exit 1; }
OUT=THIRD-PARTY-LICENSES.md
cargo about generate --fail packaging/licenses.hbs > "$OUT.tmp"
fonts=$(dirname "$(cargo metadata --format-version 1 | python3 -c 'import json,sys; print(next(p["manifest_path"] for p in json.load(sys.stdin)["packages"] if p["name"]=="epaint_default_fonts"))')")/fonts
{
    echo
    echo "## Fonts"
    echo
    echo "egui's built-in fonts are embedded in the app."
    for f in "Ubuntu-Light:UFL.txt" "Noto Emoji:OFL.txt" "Hack:Hack-Regular.txt" "emoji-icon-font:emoji-icon-font-mit-license.txt"; do
        echo
        echo "### ${f%%:*}"
        echo
        echo '```text'
        cat "$fonts/${f#*:}"
        echo '```'
    done
} >> "$OUT.tmp"
mv "$OUT.tmp" "$OUT"
echo "Wrote $OUT"
