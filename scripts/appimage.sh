#!/usr/bin/env bash
# Build the Linux AppImage: <out>/plotextract-<version>-<platform>.AppImage, one file that runs as it is.
# It runs dist.sh first, so the release app, its folder and its archive are made too.
#
#   scripts/appimage.sh                  # writes to ../dist, next to the repository
#   scripts/appimage.sh /path/to/release
#
# Needs appimagetool. Set APPIMAGETOOL to one you have; otherwise the current one is downloaded from its releases
# (https://github.com/AppImage/appimagetool) into <out>/tools.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="${1:-$repo/../dist}"

cd "$repo"
bash "$repo/scripts/dist.sh" "$out"
out="$(cd "$out" && pwd)"

version="$(cargo metadata --no-deps --format-version 1 |
  python3 -c 'import sys, json; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "plotextract"))')"
host="$(rustc -vV | sed -n 's/^host: //p')"
case "$host" in
  x86_64-*linux*)   platform=linux-x86_64;  arch=x86_64 ;;
  aarch64-*linux*)  platform=linux-aarch64; arch=aarch64 ;;
  *) echo "an AppImage is for Linux; the host is $host" >&2; exit 1 ;;
esac

tool="${APPIMAGETOOL:-}"
if [[ -z "$tool" ]]; then
  tool="$out/tools/appimagetool-$arch.AppImage"
  if [[ ! -x "$tool" ]]; then
    mkdir -p "$out/tools"
    curl --fail --location --silent --show-error --output "$tool" \
      "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$arch.AppImage"
    chmod +x "$tool"
  fi
fi

# The AppDir: the program, what a desktop needs to list it (a .desktop file and an icon), and AppRun, which starts it.
appdir="$out/plotextract.AppDir"
rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/256x256/apps" \
  "$appdir/usr/share/doc/plotextract"
cp "$out/$platform/plotextract" "$appdir/usr/bin/"
cp "$out/$platform/README.txt" "$out/$platform/LICENSE-MIT" "$out/$platform/LICENSE-APACHE" \
  "$appdir/usr/share/doc/plotextract/"
cp "$repo/crates/ui/assets/icon.png" "$appdir/usr/share/icons/hicolor/256x256/apps/plotextract.png"
cp "$repo/crates/ui/assets/icon.png" "$appdir/plotextract.png"
ln -s plotextract.png "$appdir/.DirIcon"
cat >"$appdir/plotextract.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=plotextract
Comment=Pick curves from plots and export them as data
Exec=plotextract %f
Icon=plotextract
Terminal=false
Categories=Science;DataVisualization;
MimeType=application/pdf;image/svg+xml;application/postscript;image/x-eps;image/png;image/jpeg;image/gif;image/bmp;image/tiff;image/webp;
X-AppImage-Version=$version
DESKTOP
cp "$appdir/plotextract.desktop" "$appdir/usr/share/applications/"
ln -s usr/bin/plotextract "$appdir/AppRun"

image="$out/plotextract-$version-$platform.AppImage"
rm -f "$image"
# extract-and-run: appimagetool is an AppImage itself, and a build machine may have no FUSE to mount it with
ARCH="$arch" "$tool" --appimage-extract-and-run --no-appstream "$appdir" "$image"
chmod +x "$image"
rm -rf "$appdir"

ls -l "$image"
echo "wrote $image"
