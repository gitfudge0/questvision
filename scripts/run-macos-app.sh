#!/bin/bash
set -eu

fail() {
    printf '%s\n' "$*" >&2
    exit 1
}

[[ $(uname -s) == Darwin ]] || fail 'macOS app launching requires macOS.'
activate_only=false
if [[ ${1-} == --activate-if-running ]]; then
    activate_only=true
    shift
fi
[[ $# -ge 1 ]] || fail 'Usage: run-macos-app.sh [--activate-if-running] <bundle.app> [command and arguments...]'
requested_bundle=$1
shift
bundle_id=io.github.gitfudge0.questdisplay

find_app() {
    /usr/bin/lsappinfo -nonames find "bundleid=$bundle_id"
}

app=$(find_app) || fail 'Could not inspect running apps.'
# Exit 3 tells the Makefile that it can build a fresh bundle. This also works
# before the first build, when requested_bundle does not exist yet.
if [[ -z "$app" && $activate_only == true ]]; then
    exit 3
fi

bundle=$(CDPATH= cd -- "$requested_bundle" && pwd -P) || fail 'The app bundle does not exist.'
executable="$bundle/Contents/MacOS/questdisplay"
[[ -f "$executable" && -x "$executable" ]] || fail 'The bundled executable is missing.'
[[ $(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$bundle/Contents/Info.plist") == "$bundle_id" ]] || fail 'Unexpected app bundle identifier.'

if [[ -n "$app" ]]; then
    [[ $# -eq 1 && $1 == gui ]] || fail 'Quest Display is already running; stop it before changing its command or arguments.'
    [[ $app =~ ^ASN:0x[[:xdigit:]]+-0x[[:xdigit:]]+:$ ]] || fail 'Could not identify a single running Quest Display app.'
    path_info=$(/usr/bin/lsappinfo info -only executablepath "$app") || fail 'Could not inspect the running app executable.'
    [[ $path_info == "\"CFBundleExecutablePath\"=\"$executable\"" ]] || fail 'Quest Display is running from another bundle; activate or stop that app first.'
    pid_info=$(/usr/bin/lsappinfo info -only pid "$app") || fail 'Could not inspect the running app PID.'
    [[ $pid_info =~ ^\"pid\"=([0-9]+)$ && ${BASH_REMATCH[1]} -gt 1 ]] || fail 'Could not identify the running app PID.'
    app_pid=${BASH_REMATCH[1]}
    # Activate the registered process directly, avoiding open's reopen event,
    # which can time out while the existing app is handling a permission dialog.
    if activation=$(/usr/bin/osascript -l JavaScript -e '
ObjC.import("AppKit");
function run(argv) {
    var app = $.NSRunningApplication.runningApplicationWithProcessIdentifier(Number(argv[0]));
    if (app.isNil() || ObjC.unwrap(app.bundleIdentifier) !== argv[1] ||
        ObjC.unwrap(app.executableURL.path) !== argv[2]) return false;
    return app.activateWithOptions($.NSApplicationActivateAllWindows);
}' "$app_pid" "$bundle_id" "$executable" 2>/dev/null) && [[ $activation == true ]]; then
        printf '%s\n' 'Quest Display is already running; activated the existing app.'
        exit 0
    fi
    if open_output=$(/usr/bin/open -a "$bundle" 2>&1); then
        printf '%s\n' 'Quest Display is already running; activated the existing app.'
        exit 0
    fi
    if [[ $open_output =~ error[[:space:]]+-1712([[:space:].]|$) ]]; then
        # A timeout is harmless only while the same app is still registered.
        current_pid=$(/usr/bin/lsappinfo info -only pid "$app") || fail 'Could not inspect the app after activation timed out.'
        [[ $current_pid == "$pid_info" ]] || fail 'Quest Display exited while activation timed out.'
        printf '%s\n' 'Quest Display is already running; activation timed out. Use its Dock icon to bring it forward.'
        exit 0
    fi
    [[ -z "$open_output" ]] || printf '%s\n' "$open_output" >&2
    fail 'Could not activate the running Quest Display app.'
fi

tty_path=$(tty) || fail 'App launching requires an interactive terminal for output and Ctrl-C handling.'
[[ -t 0 && -t 1 && -t 2 ]] || fail 'App launching requires an interactive terminal for output and Ctrl-C handling.'

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
