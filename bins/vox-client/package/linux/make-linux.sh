#!/usr/bin/env bash
# Builds the Linux downloads of vox-client: a tar.gz with the program, its data and a
# README, and an AppImage whose AppRun starts the same program from usr/share/vox.
# Usage: make-linux.sh <version> <binary> <outdir>
set -euo pipefail

if [ "$#" -ne 3 ]; then
    echo "usage: $0 <version> <binary> <outdir>" >&2
    exit 2
fi
version="$1"
binary="$(readlink -f "$2")"
[ -f "$binary" ] || { echo "no such binary: $binary" >&2; exit 1; }
mkdir -p "$3"
outdir="$(cd "$3" && pwd)"

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../../.." && pwd)"
name="Vox-$version-linux-x86_64"

# Everything is staged here so the repository is never written to; removed on exit.
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# tar.gz: one folder, so the player can unpack it anywhere and run vox-client inside.
stage="$work/$name"
mkdir -p "$stage"
install -m 755 "$binary" "$stage/vox-client"
cp -r "$root/assets" "$root/data" "$stage/"
cp "$root/docs/help/getting-started.md" "$stage/README.md"
tar -C "$work" -czf "$outdir/$name.tar.gz" "$name"

# AppDir: the layout vox-client looks for (usr/bin beside usr/share/vox).
appdir="$work/Vox.AppDir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/vox" \
    "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/256x256/apps"
install -m 755 "$binary" "$appdir/usr/bin/vox-client"
cp -r "$root/assets" "$root/data" "$appdir/usr/share/vox/"
install -m 644 "$here/vox.desktop" "$appdir/usr/share/applications/vox.desktop"
install -m 644 "$root/assets/icon.png" "$appdir/usr/share/icons/hicolor/256x256/apps/vox.png"
install -m 644 "$here/vox.desktop" "$appdir/vox.desktop"
install -m 644 "$root/assets/icon.png" "$appdir/vox.png"
ln -s vox.png "$appdir/.DirIcon"
cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
# AppImage entry point: run the game from this image's usr/bin.
APPDIR="${APPDIR:-$(dirname "$(readlink -f "$0")")}"
exec "$APPDIR/usr/bin/vox-client" "$@"
EOF
chmod +x "$appdir/AppRun"

# appimagetool: use the one given in $APPIMAGETOOL, else fetch the current release.
tool="${APPIMAGETOOL:-}"
if [ -z "$tool" ]; then
    # Pinned release, checked before it runs: a moving download must not run unverified.
    tool="$work/appimagetool"
    curl -fL --retry 3 -o "$tool" \
        https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
    echo "ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0  $tool" | sha256sum -c -
    chmod +x "$tool"
fi
image="$outdir/$name.AppImage"
rm -f "$image"
# Extract-and-run works without FUSE, which CI runners and containers often lack.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$tool" "$appdir" "$image"

echo "tarball:  $outdir/$name.tar.gz"
echo "AppImage: $image"
