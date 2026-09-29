#!/bin/sh
set -eu

fail() {
    printf '%s\n' "$*" >&2
    exit 1
}

select_signing_identity() (
    if [ "${CODE_SIGN_IDENTITY+x}" = x ]; then
        [ -n "$CODE_SIGN_IDENTITY" ] || fail 'CODE_SIGN_IDENTITY is empty; set it to a signing identity or - for ad-hoc signing.'
        printf '%s\n' "$CODE_SIGN_IDENTITY"
        exit 0
    fi
    if [ ! -e "$1" ]; then
        printf '%s\n' '-'
        exit 0
    fi

    signature=$(codesign -d --verbose=2 "$1" 2>&1) || fail 'Cannot inspect the existing app signature; set CODE_SIGN_IDENTITY explicitly.'
    case "$signature" in
        *'Signature=adhoc'*)
            printf '%s\n' '-'
            exit 0
            ;;
        *'Authority='*) ;;
        *) fail 'Cannot identify the existing app signer; set CODE_SIGN_IDENTITY explicitly.' ;;
    esac

    certificate_dir=$(mktemp -d "${TMPDIR:-/tmp}/questdisplay-signing.XXXXXX") || fail 'Could not create a signing inspection directory.'
    trap 'rm -rf -- "$certificate_dir"' 0
    trap 'exit 1' HUP INT TERM
    codesign -d --extract-certificates="$certificate_dir/cert" "$1" >/dev/null 2>&1 || fail 'Could not extract the existing app signing certificate.'
    [ -f "$certificate_dir/cert0" ] || fail 'The existing app signing certificate is missing.'
    fingerprint=$(shasum -a 1 "$certificate_dir/cert0" | awk '{print toupper($1)}')
    [ "${#fingerprint}" -eq 40 ] || fail 'Could not read the existing signing certificate fingerprint.'
    identities=$(security find-identity -v -p codesigning) || fail 'Could not inspect valid code signing identities in the keychain.'
    matching_identity=$(printf '%s\n' "$identities" | awk -v fingerprint="$fingerprint" '$2 == fingerprint {print $2; exit}')
    [ -n "$matching_identity" ] || fail "The existing app signer $fingerprint is not available as a valid keychain identity; restore its certificate and private key or set CODE_SIGN_IDENTITY explicitly."
    printf '%s\n' "$matching_identity"
)

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
signing_identity=$(select_signing_identity "$bundle") || fail 'Could not select a signing identity; the app bundle was left unchanged.'

mkdir -p "$bundle/Contents/MacOS"
cp "$plist" "$bundle/Contents/Info.plist"
cp "$binary" "$executable"
chmod 755 "$executable"
plutil -lint "$bundle/Contents/Info.plist"
codesign --force --sign "$signing_identity" "$bundle"
codesign --verify --strict "$bundle"
printf 'Created %s\n' "$bundle"
