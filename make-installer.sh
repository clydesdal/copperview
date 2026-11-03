#!/bin/sh
# Builds dist/install-copperview-<version>.sh: one small file that holds the
# whole program and installs it when run.
set -e
cd "$(dirname "$0")"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
cargo build --release

mkdir -p dist
out="dist/install-copperview-$version.sh"
cat > "$out" <<'HEADER'
#!/bin/sh
# Copperview installer. Run it with:  sh install-copperview.sh
# To remove the app later, run:       copperview uninstall
set -e
program="$HOME/Applications/copperview"
mkdir -p "$HOME/Applications"
start=$(awk '/^__PROGRAM__$/ { print NR + 1; exit }' "$0")
tail -n +"$start" "$0" | gzip -dc > "$program.new"
chmod +x "$program.new"
mv -f "$program.new" "$program"
"$program" install
exit 0
__PROGRAM__
HEADER
gzip -9c target/release/copperview >> "$out"
chmod +x "$out"
echo "$out"
