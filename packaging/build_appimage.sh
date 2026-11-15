#!/usr/bin/env bash
# Build Sofa-Remote-x86_64.AppImage.
set -euo pipefail
cd "$(dirname "$0")/.."
TOOL="${APPIMAGETOOL:-$(command -v appimagetool || echo "$HOME/.cache/kraken-rgb-tools/appimagetool-x86_64.AppImage")}"
[ -x "$TOOL" ] || { echo "Set APPIMAGETOOL to an appimagetool binary"; exit 1; }
cargo build --release
APPDIR=target/AppDir
rm -rf "$APPDIR" && mkdir -p "$APPDIR/usr/bin"
install -m755 target/release/sofa-remote "$APPDIR/usr/bin/"
install -m755 packaging/AppRun "$APPDIR/AppRun"
cp packaging/io.github.billy1204.sofa-remote.desktop "$APPDIR/"
cp assets/icon.png "$APPDIR/io.github.billy1204.sofa-remote.png"
ln -s io.github.billy1204.sofa-remote.png "$APPDIR/.DirIcon"
install -Dm644 assets/icon.svg "$APPDIR/usr/share/icons/hicolor/scalable/apps/io.github.billy1204.sofa-remote.svg"
install -Dm644 assets/icon.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/io.github.billy1204.sofa-remote.png"
install -Dm644 packaging/io.github.billy1204.sofa-remote.desktop "$APPDIR/usr/share/applications/io.github.billy1204.sofa-remote.desktop"
install -Dm644 packaging/io.github.billy1204.sofa-remote.metainfo.xml "$APPDIR/usr/share/metainfo/io.github.billy1204.sofa-remote.metainfo.xml"
install -Dm644 -t "$APPDIR/usr/share/doc/sofa-remote" LICENSE THIRD-PARTY-LICENSES.md
OUT=Sofa-Remote-x86_64.AppImage
rm -f "$OUT.tmp"
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$TOOL" --no-appstream "$APPDIR" "$OUT.tmp"
mv -f "$OUT.tmp" "$OUT"
echo "Built $OUT"
