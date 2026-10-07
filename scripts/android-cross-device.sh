#!/usr/bin/env bash
# Cross-device on a real Android runtime: the IGRIS APK on the emulator pairs
# with a real IGRIS device hub on the CI host ("CI PC", examples/device_peer.rs)
# through a real relay (igris-relay), using only the phone's own UI:
# connect → join with the code → send a task → approve the PC's action with a
# signed approval → see "Done — completed on CI PC". Runs after
# android-smoke.sh (the app is installed and open). Artifacts: android-artifacts/.
set -uo pipefail

PKG=dev.igris.app
OUT=android-artifacts/cross-device
PORT=8787
mkdir -p "$OUT"

target/debug/igris-relay --listen "127.0.0.1:$PORT" > "$OUT/relay.log" 2>&1 &
RELAY=$!
sleep 1
target/debug/examples/device_peer "ws://127.0.0.1:$PORT" "$OUT/peer" > /dev/null 2>&1 &
PEER=$!
# The phone reaches the host's relay at its own 127.0.0.1 (loopback, so ws:// is allowed).
adb reverse "tcp:$PORT" "tcp:$PORT"
trap 'kill $RELAY $PEER 2>/dev/null' EXIT

dump() {
  adb shell uiautomator dump /sdcard/x.xml > /dev/null 2>&1
  adb pull /sdcard/x.xml "$OUT/ui.xml" > /dev/null 2>&1
}

H=$(adb shell wm size | grep -o '[0-9]*x[0-9]*' | tail -1 | cut -dx -f2)
H=${H:-1920}

# Tap the node whose text, content-desc or hint is exactly $1 (else the first containing it).
tap() {
  for _ in $(seq 1 14); do
    dump
    xy=$(python3 - "$1" "$OUT/ui.xml" <<'PY'
import re, sys
want, path = sys.argv[1], sys.argv[2]
xml = open(path, encoding="utf-8", errors="replace").read()
nodes = [dict(re.findall(r'([\w-]+)="([^"]*)"', n)) for n in re.findall(r"<node [^>]*>", xml)]
def center(a):
    b = re.findall(r"\d+", a.get("bounds", ""))
    if len(b) == 4:
        x1, y1, x2, y2 = map(int, b)
        if x2 > x1 and y2 > y1:
            return f"{(x1 + x2) // 2} {(y1 + y2) // 2}"
keys = ("text", "content-desc", "hint")
for match in (lambda v: v.strip() == want, lambda v: want in v):
    hit = next((c for a in nodes if any(match(a.get(k, "")) for k in keys) for c in [center(a)] if c), None)
    if hit:
        print(hit)
        break
PY
)
    if [ -n "$xy" ]; then
      y=${xy#* }
      # Off screen (below the fold, or under the keyboard / tab bar): scroll it into view.
      if [ "$y" -gt $((H * 3 / 4)) ]; then
        adb shell input swipe 500 $((H * 7 / 10)) 500 $((H * 3 / 10)) 400
        sleep 1
        continue
      elif [ "$y" -lt $((H / 12)) ]; then
        adb shell input swipe 500 $((H * 3 / 10)) 500 $((H * 7 / 10)) 400
        sleep 1
        continue
      fi
      adb shell input tap $xy
      sleep 1
      return 0
    fi
    sleep 2
  done
  echo "::error::Couldn't find \"$1\" on screen"
  return 1
}

type_text() { adb shell input text "$(printf '%s' "$1" | sed 's/ /%s/g')"; sleep 1; }

# Wait (up to $2 s) until the screen shows $1.
wait_screen() {
  for _ in $(seq 1 $(($2 / 3))); do
    dump
    grep -q "$1" "$OUT/ui.xml" && return 0
    sleep 3
  done
  return 1
}

wait_file() { for _ in $(seq 1 "$2"); do [ -s "$1" ] && return 0; sleep 1; done; return 1; }

fail=0
check() { if eval "$2"; then echo "PASS: $1"; else echo "::error::FAIL: $1"; fail=1; fi; }

wait_file "$OUT/peer/code.txt" 60
CODE=$(cat "$OUT/peer/code.txt" 2>/dev/null)
check "CI PC is connected and shows a pairing code" '[ -n "$CODE" ]'

adb shell am start -n "$PKG/.MainActivity" > /dev/null
sleep 3
tap "More" && tap "Phone and computer"
tap "wss://relay.example.com" && type_text "ws://127.0.0.1:$PORT"
adb shell input keyevent 111
tap "Connect"
check "phone connected to the relay" 'wait_screen "Connected" 45'
tap "XXXX-XXXX" && type_text "$CODE"
adb shell input keyevent 111
tap "Join"
check "the PC allowed the phone (pairing)" 'for _ in $(seq 1 60); do grep -q "^PAIRED " "$OUT/peer/events.log" && break; sleep 1; done; grep -q "^PAIRED Pixel\|^PAIRED My phone\|^PAIRED " "$OUT/peer/events.log"'
check "phone lists CI PC as paired" 'wait_screen "Send to CI PC" 60'
adb exec-out screencap -p > "$OUT/paired.png" || true

tap "Open Notepad" && type_text "say hello from the emulator"
adb shell input keyevent 111
tap "Send to CI PC"
check "CI PC received the task" 'for _ in $(seq 1 60); do grep -q "^TASK_RECEIVED" "$OUT/peer/events.log" && break; sleep 1; done; grep -q "^TASK_RECEIVED from=.* objective=say hello from the emulator" "$OUT/peer/events.log"'
check "phone asks to approve the PC's action" 'wait_screen "CI PC needs your OK" 60'
adb exec-out screencap -p > "$OUT/approval.png" || true
tap "Approve"
check "PC verified the phone's signed approval" 'for _ in $(seq 1 60); do grep -q "^APPROVAL" "$OUT/peer/events.log" && break; sleep 1; done; grep -q "^APPROVAL Approved" "$OUT/peer/events.log"'
check "phone shows the real outcome (Done — completed on CI PC)" 'wait_screen "Done — completed on CI PC" 60'
adb exec-out screencap -p > "$OUT/done.png" || true

log=$(adb shell run-as "$PKG" find . -name 'igris.log*' 2>/dev/null | tr -d '\r' | head -1)
[ -n "$log" ] && adb shell run-as "$PKG" cat "$log" > "$OUT/igris.log"
echo "--- phone device events ---"
grep -o '"event":"\(DEVICE\|REMOTE\|PAIRING\)[A-Z_]*"[^}]\{0,120\}' "$OUT/igris.log" 2>/dev/null | head -30 || true
echo "--- CI PC events ---"
grep -v '^EVENT' "$OUT/peer/events.log" || true
exit $fail
