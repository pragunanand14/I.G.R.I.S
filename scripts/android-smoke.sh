#!/usr/bin/env bash
# Install the IGRIS APK on a running emulator/device, launch it, and check
# from IGRIS's own log that it started on Android with its phone tools and
# that the device plugin answered. Artifacts go to android-artifacts/.
set -uo pipefail

PKG=dev.igris.app
OUT=android-artifacts
EXPECTED_TOOLS=25
mkdir -p "$OUT"

apk=$(find src-tauri/gen/android/app/build/outputs/apk -name '*.apk' | head -1)
[ -n "$apk" ] || { echo "::error::No APK was built"; exit 1; }
echo "APK: $apk ($(du -h "$apk" | cut -f1))"

adb install -r "$apk" || { echo "::error::Install failed"; exit 1; }
adb logcat -c || true
adb shell am start -W -n "$PKG/.MainActivity"
sleep 30

pid=$(adb shell pidof "$PKG" | tr -d '\r' || true)
# What is on screen. The webview builds its accessibility tree on first use,
# so dump once to wake it, wait, and dump again.
adb shell uiautomator dump /sdcard/igris-ui.xml >/dev/null 2>&1 || true
sleep 5
adb shell uiautomator dump /sdcard/igris-ui.xml 2>&1 | tail -1
adb pull /sdcard/igris-ui.xml "$OUT/ui.xml" >/dev/null 2>&1 || : > "$OUT/ui.xml"
echo "--- on-screen text ($(wc -c < "$OUT/ui.xml") bytes of UI dump) ---"
grep -o -E '(text|content-desc)="[^"]+"' "$OUT/ui.xml" | sort -u | head -40 || true
adb exec-out screencap -p > "$OUT/screen.png" || true
adb logcat -d > "$OUT/logcat.txt" || true
log=$(adb shell run-as "$PKG" find . -name 'igris.log*' 2>/dev/null | tr -d '\r' | head -1)
if [ -n "$log" ]; then
  adb shell run-as "$PKG" cat "$log" > "$OUT/igris.log"
else
  echo "::warning::IGRIS log file not found in the app's data directory"
  : > "$OUT/igris.log"
fi

echo "--- IGRIS events ---"
grep -o '"event":"[A-Z_]*"[^}]\{0,160\}' "$OUT/igris.log" || true
echo "--- crashes in logcat ---"
grep -E "FATAL EXCEPTION|panicked|SIGSEGV|SIGABRT" "$OUT/logcat.txt" | head -20 || true

fail=0
check() { if eval "$2"; then echo "PASS: $1"; else echo "::error::FAIL: $1"; fail=1; fi; }
check "app process is running (pid ${pid:-none})" '[ -n "$pid" ]'
check "IGRIS started (APP_STARTED)" 'grep -q "\"event\":\"APP_STARTED\"" "$OUT/igris.log"'
check "phone tool set registered ($EXPECTED_TOOLS tools)" 'grep -q "\"event\":\"TOOLS_REGISTERED\",\"count\":$EXPECTED_TOOLS" "$OUT/igris.log"'
check "device plugin answered (PHONE_READY)" 'grep -q "\"event\":\"PHONE_READY\"" "$OUT/igris.log"'
# Text only the backend can supply proves the UI is up and its IPC reaches Rust.
check "UI rendered and reached the backend (AI status from Rust on screen)" 'grep -q "No AI provider configured" "$OUT/ui.xml"'
check "UI did not report the backend as unavailable" '! grep -q "backend is unavailable" "$OUT/ui.xml"'
# The phone UI must not show desktop-only things or zeros it can't measure.
check "no desktop hotkey hint on the phone" '! grep -q "Ctrl+Shift+Space" "$OUT/ui.xml"'
check "no unmeasured CPU zeros (\"0 threads\")" '! grep -q "\"0 threads\"" "$OUT/ui.xml"'
check "no crash in logcat" '! grep -qE "FATAL EXCEPTION.*|panicked at" "$OUT/logcat.txt"'
exit $fail
