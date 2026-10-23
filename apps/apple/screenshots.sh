#!/bin/bash
# Take screenshots of the Mac app, so each build can be looked at before
# installing it: the demo with a conversation open, then the welcome screen
# of a throwaway session that saves nothing.
#
# usage: apps/apple/screenshots.sh <MailHerd.app> <out-dir>
set -u
APP=${1:-dist/MailHerd.app}
OUT=${2:-dist/mac-screenshots}
BIN="$APP/Contents/MacOS/MailHerd"
mkdir -p "$OUT"

shoot() {
    local name=$1
    shift
    env "$@" "$BIN" > "$OUT/$name.log" 2>&1 &
    local pid=$!
    sleep 8
    osascript -e 'tell application "System Events" to set frontmost of (first process whose unix id is '"$pid"') to true' 2>/dev/null || true
    sleep 1
    screencapture -x "$OUT/$name.png" || echo "screencapture failed for $name"
    kill "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
}

shoot 1-demo MAILHERD_DEMO=1 MAILHERD_OPEN_FIRST=1
shoot 2-welcome MAILHERD_EPHEMERAL=1
exit 0
