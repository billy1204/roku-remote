#!/usr/bin/env bash
# Regenerate THIRD-PARTY-LICENSES.md: the notices of every crate in the
# Linux and Windows builds (via cargo-about), then those of the bundled
# fonts, Rust's standard library and the AppImage runtime (the last two
# kept in packaging/notices, copied from each project's repository).
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
    notice() {
        echo
        echo "### $1"
        echo
        echo '```text'
        cat "packaging/notices/$2"
        echo '```'
    }
    echo
    echo "## Rust standard library"
    echo
    echo "Every Rust program contains Rust's standard library, under MIT or Apache-2.0 (the Apache text is above)."
    notice "Copyright" rust-COPYRIGHT
    notice "MIT License" rust-LICENSE-MIT
    echo
    echo "## AppImage runtime"
    echo
    echo "The Linux AppImage starts with the AppImage type 2 runtime, which contains the components below."
    echo "Its source, including the libfuse library under the LGPL, is at https://github.com/AppImage/type2-runtime;"
    echo "libfuse's own source is at https://github.com/libfuse/libfuse. Run the AppImage with --appimage-version to see"
    echo "which runtime build it contains."
    notice "type2-runtime (MIT)" appimage-runtime-LICENSE
    notice "musl libc (MIT)" musl-COPYRIGHT
    notice "squashfuse (BSD-2-Clause)" squashfuse-LICENSE
    notice "zstd (BSD-3-Clause)" zstd-LICENSE
    notice "libfuse (LGPL-2.1)" libfuse-LGPL2.txt
} >> "$OUT.tmp"
mv "$OUT.tmp" "$OUT"
echo "Wrote $OUT"
