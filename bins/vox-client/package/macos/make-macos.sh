#!/usr/bin/env bash
# Builds Vox.app (universal: arm64 + x86_64) and Vox-<version>-macos-universal.dmg.
#
# Usage: make-macos.sh <version> <arm64-binary> <x86_64-binary> <outdir>
#   e.g. make-macos.sh 0.1.0 target/aarch64-apple-darwin/release/vox-client \
#        target/x86_64-apple-darwin/release/vox-client dist
#
# Run from anywhere; the repo root is found from this script's location.
# Signing: if MACOS_SIGN_IDENTITY is set, the app and the dmg are signed with it
# (hardened runtime, timestamp). Otherwise the app is ad-hoc signed and the dmg is not.
# Notarization is not done here; the release workflow does it.
set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <version> <arm64-binary> <x86_64-binary> <outdir>" >&2
  exit 2
fi

version="$1"
arm_bin="$2"
x86_bin="$3"
outdir="$4"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# package/macos -> package -> vox-client -> bins -> repo root
repo="$(cd "$script_dir/../../../.." && pwd)"

for f in "$arm_bin" "$x86_bin"; do
  if [ ! -f "$f" ]; then
    echo "missing binary: $f" >&2
    exit 1
  fi
done
for d in assets data; do
  if [ ! -d "$repo/$d" ]; then
    echo "missing folder: $repo/$d" >&2
    exit 1
  fi
done
if [ ! -f "$repo/assets/icon.png" ]; then
  echo "missing icon: $repo/assets/icon.png" >&2
  exit 1
fi

# The bundle versions must be numeric x.y.z; anything else (e.g. a branch name) becomes 0.0.0.
if [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  bundle_version="$version"
else
  echo "warning: version '$version' is not x.y.z; bundle version set to 0.0.0" >&2
  bundle_version="0.0.0"
fi

mkdir -p "$outdir"
outdir="$(cd "$outdir" && pwd)"
app="$outdir/Vox.app"
staging="$outdir/dmg-staging"
dmg="$outdir/Vox-$version-macos-universal.dmg"

rm -rf "$app" "$staging" "$dmg"
contents="$app/Contents"
mkdir -p "$contents/MacOS" "$contents/Resources"

# One binary for both Macs.
lipo -create -output "$contents/MacOS/vox-client" "$arm_bin" "$x86_bin"
chmod +x "$contents/MacOS/vox-client"

# Placed in Contents/Resources. find_root (bins/vox-client/src/main.rs) looks in Contents/Resources.
cp -R "$repo/assets" "$contents/Resources/assets"
cp -R "$repo/data" "$contents/Resources/data"

sed "s/@VERSION@/$bundle_version/g" "$script_dir/Info.plist" > "$contents/Info.plist"

# Icon: a 1x and 2x set from the 256 px source, then iconutil makes Vox.icns.
iconset="$outdir/Vox.iconset"
rm -rf "$iconset"
mkdir -p "$iconset"
icon_src="$repo/assets/icon.png"
sips -z 16 16 "$icon_src" --out "$iconset/icon_16x16.png" >/dev/null
sips -z 32 32 "$icon_src" --out "$iconset/icon_16x16@2x.png" >/dev/null
sips -z 32 32 "$icon_src" --out "$iconset/icon_32x32.png" >/dev/null
sips -z 64 64 "$icon_src" --out "$iconset/icon_32x32@2x.png" >/dev/null
sips -z 128 128 "$icon_src" --out "$iconset/icon_128x128.png" >/dev/null
sips -z 256 256 "$icon_src" --out "$iconset/icon_128x128@2x.png" >/dev/null
sips -z 256 256 "$icon_src" --out "$iconset/icon_256x256.png" >/dev/null
iconutil -c icns "$iconset" -o "$contents/Resources/Vox.icns"
rm -rf "$iconset"

# Sign the app. The inside-out order is not needed: the bundle has no nested code.
if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
  codesign --force --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY" "$app"
else
  codesign --force --sign - "$app"
fi

# The disk image: Vox.app next to a link to /Applications, so the user can drag it across.
mkdir -p "$staging"
cp -R "$app" "$staging/Vox.app"
ln -s /Applications "$staging/Applications"
hdiutil create -volname Vox -fs HFS+ -format UDZO -srcfolder "$staging" -ov "$dmg" >/dev/null
rm -rf "$staging"

if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
  codesign --force --timestamp --sign "$MACOS_SIGN_IDENTITY" "$dmg"
fi

echo "$dmg"
