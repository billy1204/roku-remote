#!/usr/bin/env bash
# Build Sofa-Remote-x86_64.flatpak from the release the manifest names.
# Needs: flatpak install --user flathub org.flatpak.Builder
set -euo pipefail
cd "$(dirname "$0")/.."
ID=io.github.billy1204.sofa-remote
WORK="$PWD/target/flatpak"
mkdir -p "$WORK"
# The builder asks the host's flatpak for the SDK; point it at the real user
# installation instead of the builder's own sandboxed data folder.
flatpak run --env=FLATPAK_USER_DIR="${FLATPAK_USER_DIR:-$HOME/.local/share/flatpak}" \
    --command=flatpak-builder org.flatpak.Builder --user --force-clean \
    --install-deps-from=flathub --state-dir="$WORK/state" --repo="$WORK/repo" \
    "$WORK/build" "packaging/flatpak/$ID.yml"
OUT=Sofa-Remote-x86_64.flatpak
rm -f "$OUT.tmp"
flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo "$WORK/repo" "$OUT.tmp" "$ID"
mv -f "$OUT.tmp" "$OUT"
echo "Built $OUT"
