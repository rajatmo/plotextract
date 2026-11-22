#!/usr/bin/env bash
# Build the macOS disk image: <out>/plotextract-<version>-<platform>.dmg. Opened, it shows the app beside a link to
# Applications, to drag it there. It runs dist.sh first, so the release app, its folder and its archive are made too.
#
#   scripts/dmg.sh                  # writes to ../dist, next to the repository
#   scripts/dmg.sh /path/to/release
#
# The app is unsigned, as in the archive: the first time, right-click it and choose Open.
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
  x86_64-apple-*)   platform=macos-x86_64 ;;
  aarch64-apple-*)  platform=macos-aarch64 ;;
  *) echo "a disk image is for macOS; the host is $host" >&2; exit 1 ;;
esac

# what the image holds: the app, the link to drag it onto, and the texts
stage="$out/plotextract.dmg.dir"
rm -rf "$stage"
mkdir -p "$stage"
cp -R "$out/$platform/plotextract.app" "$stage/"
ln -s /Applications "$stage/Applications"
cp "$out/$platform/README.txt" "$out/$platform/LICENSE-MIT" "$out/$platform/LICENSE-APACHE" "$stage/"

image="$out/plotextract-$version-$platform.dmg"
rm -f "$image"
# hdiutil now and then fails with "Resource busy" on a build machine: try a few times
for attempt in 1 2 3 4 5; do
  if hdiutil create -volname "plotextract $version" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$image"; then
    break
  fi
  if [[ "$attempt" == 5 ]]; then
    echo "hdiutil could not make $image" >&2
    exit 1
  fi
  sleep 5
done
rm -rf "$stage"

ls -l "$image"
echo "wrote $image"
