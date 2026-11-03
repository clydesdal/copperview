#!/bin/sh
# Builds dist/Copperview.app: the program, its icon and the Info.plist that makes
# it an app you can drag into Applications. Run it on a Mac.
set -e
cd "$(dirname "$0")"

[ "$(uname -s)" = "Darwin" ] || { echo "This builds the Mac app, so it has to run on a Mac." >&2; exit 1; }
command -v iconutil >/dev/null || { echo "iconutil is needed, and comes with macOS." >&2; exit 1; }

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
cargo build --release

app="dist/Copperview.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/copperview "$app/Contents/MacOS/copperview"
sed "s/__VERSION__/$version/g" assets/macos/Info.plist.in > "$app/Contents/Info.plist"

# The icon: the pictures in assets/icons, named the way iconutil wants them.
set_dir="$(mktemp -d)/Copperview.iconset"
mkdir -p "$set_dir"
cp assets/icons/16.png  "$set_dir/icon_16x16.png"
cp assets/icons/32.png  "$set_dir/icon_16x16@2x.png"
cp assets/icons/32.png  "$set_dir/icon_32x32.png"
cp assets/icons/64.png  "$set_dir/icon_32x32@2x.png"
cp assets/icons/128.png "$set_dir/icon_128x128.png"
cp assets/icons/256.png "$set_dir/icon_128x128@2x.png"
cp assets/icons/256.png "$set_dir/icon_256x256.png"
cp assets/icons/512.png "$set_dir/icon_256x256@2x.png"
cp assets/icons/512.png "$set_dir/icon_512x512.png"
iconutil -c icns "$set_dir" -o "$app/Contents/Resources/Copperview.icns"

# Signed on the spot, with no certificate, so macOS will run it on this Mac.
codesign --force --deep --sign - "$app" >/dev/null 2>&1 || echo "Note: the app couldn't be signed; macOS may ask before opening it." >&2

echo "$app (version $version)"
