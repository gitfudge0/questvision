#!/bin/sh
set -eu

fail() {
    printf '%s\n' "$*" >&2
    exit 1
}

[ "$(uname -s)" = Darwin ] || fail 'macOS app packaging requires macOS.'
[ "$#" -eq 2 ] || fail 'Usage: package-macos-app.sh <binary> <output.app>'

binary=$1
bundle=${2%/}
case "$bundle" in
    *.app) ;;
    *) fail 'The output must be an .app bundle path.' ;;
esac
[ -f "$binary" ] && [ -x "$binary" ] || fail 'The input must be an executable file.'
[ "$binary" != "$bundle" ] || fail 'The input and output paths must differ.'
[ ! -L "$bundle" ] || fail 'Refusing to overwrite a symlinked bundle.'
[ ! -e "$bundle" ] || [ -d "$bundle" ] || fail 'The output already exists and is not a directory.'
running=$(/usr/bin/lsappinfo -nonames find bundleid=io.github.gitfudge0.questdisplay) || fail 'Could not inspect running apps.'
[ -z "$running" ] || fail 'Quest Display is running; stop it before packaging the app.'

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
plist="$script_dir/../packaging/macos/Info.plist"
plutil -lint "$plist"

# Only create directories and overwrite files owned by this packaging step.
for directory in "$bundle/Contents" "$bundle/Contents/MacOS" "$bundle/Contents/_CodeSignature"; do
    [ ! -L "$directory" ] || fail "Refusing to use symlinked directory: $directory"
    [ ! -e "$directory" ] || [ -d "$directory" ] || fail "Not a directory: $directory"
done
for file in "$bundle/Contents/Info.plist" "$bundle/Contents/MacOS/questdisplay" "$bundle/Contents/_CodeSignature/CodeResources"; do
    [ ! -L "$file" ] || fail "Refusing to overwrite symlinked file: $file"
    [ ! -e "$file" ] || [ -f "$file" ] || fail "Not a regular file: $file"
done
executable="$bundle/Contents/MacOS/questdisplay"
[ ! -e "$executable" ] || [ ! "$binary" -ef "$executable" ] || fail 'The input is already the bundled executable.'

mkdir -p "$bundle/Contents/MacOS"
cp "$plist" "$bundle/Contents/Info.plist"
cp "$binary" "$executable"
chmod 755 "$executable"
plutil -lint "$bundle/Contents/Info.plist"
codesign --force --sign "${CODE_SIGN_IDENTITY:--}" "$bundle"
codesign --verify --strict "$bundle"
printf 'Created %s\n' "$bundle"
