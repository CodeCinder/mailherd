#!/bin/bash
# Take screenshots of the app in demo mode on a virtual display, so each build
# can be looked at in the browser before installing it.
#
# usage: xvfb-run -a -s "-screen 0 1240x800x24" packaging/screenshots.sh <binary> <out-dir>
set -u
BIN=${1:-target/release/mailherd}
OUT=${2:-screenshots}
mkdir -p "$OUT"
export LIBGL_ALWAYS_SOFTWARE=1

"$BIN" > "$OUT/app.log" 2>&1 &
PID=$!
sleep 6

import -window root "$OUT/1-inbox.png"
xdotool mousemove 430 150 click 1   # group thread
sleep 1.5
import -window root "$OUT/2-group-chat.png"
xdotool mousemove 430 85 click 1    # one-to-one thread
sleep 1.5
xdotool mousemove 900 770 click 1   # reply box
xdotool type --delay 20 "Great, see you then!"
xdotool key Return
sleep 2
import -window root "$OUT/3-reply-sending.png"
xdotool key Escape
xdotool mousemove 575 27 click 1    # People view
sleep 1
xdotool mousemove 430 85 click 1
sleep 1.5
import -window root "$OUT/4-people.png"

kill "$PID" 2>/dev/null
wait "$PID" 2>/dev/null
exit 0
