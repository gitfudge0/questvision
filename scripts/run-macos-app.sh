#!/bin/bash
set -eu

fail() {
    printf '%s\n' "$*" >&2
    exit 1
}

[[ $(uname -s) == Darwin ]] || fail 'macOS app launching requires macOS.'
[[ $# -ge 1 ]] || fail 'Usage: run-macos-app.sh <bundle.app> [host arguments...]'
tty_path=$(tty) || fail 'App launching requires an interactive terminal for pairing.'
[[ -t 0 && -t 1 && -t 2 ]] || fail 'App launching requires an interactive terminal for pairing.'
bundle=$(CDPATH= cd -- "$1" && pwd -P) || fail 'The app bundle does not exist.'
shift
executable="$bundle/Contents/MacOS/questdisplay"
bundle_id=io.github.gitfudge0.questdisplay
[[ -f "$executable" && -x "$executable" ]] || fail 'The bundled host executable is missing.'
[[ $(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$bundle/Contents/Info.plist") == "$bundle_id" ]] || fail 'Unexpected app bundle identifier.'

find_app() {
    /usr/bin/lsappinfo -nonames find "bundleid=$bundle_id"
}

app=$(find_app) || fail 'Could not inspect running apps.'
[[ -z "$app" ]] || fail 'Quest Display is already running; stop it before launching another instance.'

tracked_asn=
tracked_pid=
open_pid=

# Record only one registered app with this bundle ID and the exact executable.
track_app() {
    local candidate pid_info path_info
    candidate=$(find_app) || return 1
    [[ $candidate =~ ^ASN:0x[[:xdigit:]]+-0x[[:xdigit:]]+:$ ]] || return 1
    pid_info=$(/usr/bin/lsappinfo info -only pid "$candidate") || return 1
    path_info=$(/usr/bin/lsappinfo info -only executablepath "$candidate") || return 1
    [[ $path_info == "\"CFBundleExecutablePath\"=\"$executable\"" ]] || return 1
    [[ $pid_info =~ ^\"pid\"=([0-9]+)$ ]] || return 1
    [[ ${BASH_REMATCH[1]} -gt 1 ]] || return 1
    tracked_asn=$candidate
    tracked_pid=${BASH_REMATCH[1]}
}

still_tracked() {
    [[ -n "$tracked_asn" && -n "$tracked_pid" ]] || return 1
    [[ $(/usr/bin/lsappinfo info -only pid "$tracked_asn") == "\"pid\"=$tracked_pid" ]] || return 1
    [[ $(/usr/bin/lsappinfo info -only executablepath "$tracked_asn") == "\"CFBundleExecutablePath\"=\"$executable\"" ]]
}

interrupted() {
    local status=$1 attempt
    trap '' INT TERM HUP
    # A signal can arrive while LaunchServices is still registering the app.
    if [[ -z "$tracked_pid" && -n "$open_pid" ]]; then
        for ((attempt = 0; attempt < 20; attempt++)); do
            track_app && break
            kill -0 "$open_pid" 2>/dev/null || break
            sleep 0.1
        done
    fi
    if still_tracked; then
        kill -TERM "$tracked_pid" 2>/dev/null || :
    fi
    if [[ -n "$open_pid" ]]; then
        kill -TERM "$open_pid" 2>/dev/null || :
        wait "$open_pid" 2>/dev/null || :
    fi
    exit "$status"
}

trap 'interrupted 130' INT
trap 'interrupted 143' TERM
trap 'interrupted 129' HUP
/usr/bin/open -W -n -a "$bundle" --stdin "$tty_path" --stdout "$tty_path" --stderr "$tty_path" --args "$@" &
open_pid=$!

for ((attempt = 0; attempt < 50; attempt++)); do
    track_app && break
    if ! kill -0 "$open_pid" 2>/dev/null; then
        # Commands such as --help can finish before an app PID is observed.
        status=0
        wait "$open_pid" || status=$?
        exit "$status"
    fi
    sleep 0.1
done
if [[ -z "$tracked_pid" ]]; then
    kill -TERM "$open_pid" 2>/dev/null || :
    wait "$open_pid" 2>/dev/null || :
    fail 'Could not identify the launched app safely; no app process was terminated.'
fi
status=0
wait "$open_pid" || status=$?
exit "$status"
