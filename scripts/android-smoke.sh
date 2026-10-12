#!/usr/bin/env bash
# Install the IGRIS APK on a running emulator/device, launch it, and check
# from IGRIS's own log that it started on Android with its phone tools and
# that the device plugin answered. Artifacts go to android-artifacts/.
set -uo pipefail

PKG=dev.igris.app
OUT=android-artifacts
EXPECTED_TOOLS=70
mkdir -p "$OUT"

apk=$(find src-tauri/gen/android/app/build/outputs/apk -name '*.apk' | head -1)
[ -n "$apk" ] || { echo "::error::No APK was built"; exit 1; }
echo "APK: $apk ($(du -h "$apk" | cut -f1))"

adb install -r "$apk" || { echo "::error::Install failed"; exit 1; }
# IGRIS asks for the notification permission once (reminder alarms); answer it as the user would, so the
# system dialog doesn't cover the screen this test reads.
adb shell pm grant "$PKG" android.permission.POST_NOTIFICATIONS || true
# Messages: a notification already in the shade before IGRIS's listener starts must still be seen.
adb shell "cmd notification post -t 'Mom' igris_smoke 'Dinner at 8?'" || true
adb shell cmd notification allow_listener "$PKG/dev.igris.device.IgrisNotificationListener" || true
# "Hey IGRIS": turned on as the user would (microphone allowed, the switch on), so it starts with the app.
adb shell pm grant "$PKG" android.permission.RECORD_AUDIO || true
printf '%s' '<?xml version="1.0" encoding="utf-8" standalone="yes" ?><map><boolean name="on" value="true" /></map>' > "$OUT/igris_wake.xml"
adb push "$OUT/igris_wake.xml" /data/local/tmp/igris_wake.xml >/dev/null && adb shell chmod 644 /data/local/tmp/igris_wake.xml
adb shell "run-as $PKG sh -c 'mkdir -p shared_prefs && cp /data/local/tmp/igris_wake.xml shared_prefs/igris_wake.xml'" || true
adb shell run-as "$PKG" cat shared_prefs/igris_wake.xml || true
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
check "device keys sealed by the Android Keystore (DEVICE_IDENTITY_READY)" 'grep -q "\"event\":\"DEVICE_IDENTITY_READY\",\"protector\":\"android-keystore\"" "$OUT/igris.log"'
# Text only the backend can supply proves the UI is up and its IPC reaches Rust.
check "UI rendered and reached the backend (AI status from Rust on screen)" 'grep -q "No AI provider configured" "$OUT/ui.xml"'
check "UI did not report the backend as unavailable" '! grep -q "backend is unavailable" "$OUT/ui.xml"'
# The phone UI must not show desktop-only things or zeros it can't measure.
check "no desktop hotkey hint on the phone" '! grep -q "Ctrl+Shift+Space" "$OUT/ui.xml"'
check "no unmeasured CPU zeros (\"0 threads\")" '! grep -q "\"0 threads\"" "$OUT/ui.xml"'
check "no crash in logcat" '! grep -qE "FATAL EXCEPTION.*|panicked at" "$OUT/logcat.txt"'
echo "--- messages / wake word / assistant ---"
grep -E "IGRIS_MESSAGES_LISTENER|IGRIS_WAKE" "$OUT/logcat.txt" | head -10 || true
check "message listener connected and read the notification already showing" 'grep -qE "IGRIS_MESSAGES_LISTENER connected, [1-9]" "$OUT/logcat.txt"'
check "\"Hey IGRIS\" loaded its on-phone model and is listening" 'grep -q "IGRIS_WAKE listening" "$OUT/logcat.txt"'
adb shell cmd package query-activities --brief -a android.intent.action.ASSIST > "$OUT/assist.txt" 2>&1 || true
check "IGRIS can be chosen as the phone's assistant (ASSIST)" 'grep -q "$PKG/dev.igris.device.AssistActivity" "$OUT/assist.txt"'
# The assistant gesture / "Hey IGRIS": IGRIS opens as a panel over what's on screen.
adb shell input keyevent KEYCODE_HOME || true
sleep 2
adb shell am start -a android.intent.action.ASSIST -n "$PKG/dev.igris.device.AssistActivity" || true
# Look while it's up (without an AI key it shows the problem and closes after a few seconds).
: > "$OUT/sheet.xml"
for i in 1 2 3 4; do
  sleep 1
  adb shell uiautomator dump /sdcard/igris-sheet.xml >/dev/null 2>&1 || true
  adb pull /sdcard/igris-sheet.xml "$OUT/sheet-$i.xml" >/dev/null 2>&1 && cat "$OUT/sheet-$i.xml" >> "$OUT/sheet.xml"
  grep -q "Open IGRIS" "$OUT/sheet.xml" && break
done
adb exec-out screencap -p > "$OUT/sheet.png" || true
grep -o -E '(text|content-desc)="[^"]+"' "$OUT/sheet.xml" | sort -u | head -20 || true
check "the assistant gesture opens IGRIS's panel (\"Open IGRIS\" on screen)" 'grep -q "Open IGRIS" "$OUT/sheet.xml"'
adb logcat -d > "$OUT/logcat-sheet.txt" || true
check "no crash opening the panel" '! grep -qE "FATAL EXCEPTION.*|panicked at" "$OUT/logcat-sheet.txt"'
# Leave IGRIS as a normal app for the tests that follow (the panel closes when IGRIS goes to the background).
adb shell input keyevent KEYCODE_HOME || true
sleep 3
exit $fail
