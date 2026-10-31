#!/usr/bin/env bash
# Build the release app and put it in <out>/<platform>/, with an archive beside it. Run on the platform you want to
# build for (Linux or macOS; scripts/dist.ps1 does Windows).
#
#   scripts/dist.sh                  # writes to ../dist, next to the repository
#   scripts/dist.sh /path/to/release
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="${1:-$repo/../dist}"
cd "$repo"

cargo build --release -p plotextract

meta="$(cargo metadata --no-deps --format-version 1)"
field() { python3 -c "import sys, json; m = json.load(sys.stdin); $1" <<<"$meta"; }
target="$(field 'print(m["target_directory"])')"
version="$(field 'print(next(p["version"] for p in m["packages"] if p["name"] == "plotextract"))')"

host="$(rustc -vV | sed -n 's/^host: //p')"
case "$host" in
  x86_64-*linux*)   platform=linux-x86_64 ;;
  aarch64-*linux*)  platform=linux-aarch64 ;;
  x86_64-apple-*)   platform=macos-x86_64 ;;
  aarch64-apple-*)  platform=macos-aarch64 ;;
  *) echo "unsupported host $host" >&2; exit 1 ;;
esac

dest="$out/$platform"
rm -rf "$dest"
mkdir -p "$dest"
cp "$target/release/plotextract" "$dest/"
cp "$repo/scripts/README.dist.txt" "$dest/README.txt"
cp "$repo/LICENSE-MIT" "$repo/LICENSE-APACHE" "$dest/"

if [[ "$platform" == macos-* ]]; then
  # A minimal, unsigned application bundle next to the plain binary.
  app="$dest/plotextract.app/Contents"
  mkdir -p "$app/MacOS" "$app/Resources"
  cp "$target/release/plotextract" "$app/MacOS/"
  cp "$repo/crates/app/assets/icon.icns" "$app/Resources/"
  cat >"$app/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>plotextract</string>
  <key>CFBundleDisplayName</key><string>plotextract</string>
  <key>CFBundleIdentifier</key><string>org.plotextract.app</string>
  <key>CFBundleExecutable</key><string>plotextract</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>Copyright (c) 2026 Rajat Mandal</string>
</dict>
</plist>
PLIST
fi

archive="$out/plotextract-$version-$platform.tar.gz"
rm -f "$archive"
tar -C "$dest" -czf "$archive" .

ls -l "$dest"
echo "wrote $dest and $archive"
