#!/usr/bin/env bash
# Build Roku-Remote-x86_64.AppImage.
set -euo pipefail
cd "$(dirname "$0")/.."
TOOL="${APPIMAGETOOL:-$(command -v appimagetool || echo "$HOME/.cache/kraken-rgb-tools/appimagetool-x86_64.AppImage")}"
[ -x "$TOOL" ] || { echo "Set APPIMAGETOOL to an appimagetool binary"; exit 1; }
cargo build --release
APPDIR=target/AppDir
rm -rf "$APPDIR" && mkdir -p "$APPDIR/usr/bin"
install -m755 target/release/roku-remote "$APPDIR/usr/bin/"
install -m755 packaging/AppRun "$APPDIR/AppRun"
cp packaging/roku-remote.desktop "$APPDIR/"
cp assets/icon.png "$APPDIR/roku-remote.png"
ln -s roku-remote.png "$APPDIR/.DirIcon"
install -Dm644 assets/icon.svg "$APPDIR/usr/share/icons/hicolor/scalable/apps/roku-remote.svg"
install -Dm644 assets/icon.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/roku-remote.png"
install -Dm644 packaging/roku-remote.desktop "$APPDIR/usr/share/applications/roku-remote.desktop"
install -Dm644 -t "$APPDIR/usr/share/doc/roku-remote" LICENSE THIRD-PARTY-LICENSES.md
OUT=Roku-Remote-x86_64.AppImage
rm -f "$OUT.tmp"
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$TOOL" --no-appstream "$APPDIR" "$OUT.tmp"
mv -f "$OUT.tmp" "$OUT"
echo "Built $OUT"
