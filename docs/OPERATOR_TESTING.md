# Operator testing

How IGRIS's computer operator is tested, what has actually been run, and what
has not. Results below are copied from real runs; nothing here is projected.

## Layers

| Layer | Where | Runs | What it proves |
|---|---|---|---|
| Unit + scripted end-to-end | `cargo test` (all platforms, CI) | every push | Tool schemas, permission levels, state machine, verification and recovery logic, toolset focus, crash recovery, event log. The model is scripted, the desktop is a test driver. |
| Real Chromium (DevTools) | `computer::browser` test | every `cargo test` where a Chromium is found (`IGRIS_BROWSER`, or Chrome/Edge in the usual places) | Launch with IGRIS's own profile, navigate, snapshot, click, type, read back over the real DevTools protocol. Skips itself when no browser is installed. |
| Real Windows desktop | `src-tauri/tests/windows_desktop.rs` (ignored; Windows only) | `Windows desktop` workflow (push to `claude/**` touching computer code, or manual) on a GitHub-hosted Windows runner with an active console session | Real screen capture, SendInput, UI Automation, focus, Notepad / File Explorer / VS Code launches, OCR, Chrome, takeover, pause and stop. **No AI model involved.** |
| Live operator (real provider) | `src-tauri/tests/operator_live.rs` (ignored; Windows only; `IGRIS_LIVE=1`) | `Operator live` workflow, manual, needs the repository secret `IGRIS_AI_API_KEY` | A real model, through the same ModelRouter / orchestrator / executor path as the app, driving the real desktop. |
| Manual | a person at a Windows 10/11 PC | by hand | Everything above on a real user's machine, plus things a runner can't show (overlay look, voice, a person taking over). |

### Running them

```text
# Real desktop suite (takes over mouse and keyboard; run on a machine you're not using)
cd src-tauri   # the desktop crate (the workspace root also works with -p igris)
cargo test --test windows_desktop -- --ignored --test-threads=1 --nocapture

# Live operator scenarios with a real provider
set IGRIS_LIVE=1
set AI_PROVIDER=gemini & set AI_MODEL=gemini-2.5-flash & set AI_API_KEY=...
cargo test --test operator_live -- --ignored --test-threads=1 --nocapture
# → target/operator-live-report.md
```

In GitHub: Actions → *Windows desktop* → Run workflow; Actions → *Operator
live* → Run workflow (provider + model inputs; add the `IGRIS_AI_API_KEY`
secret first — the job fails immediately without it rather than pretending).

The live harness approves ordinary approvals and **denies every CRITICAL
action**, recording each request in the report, so an unattended run can never
send, delete or purchase anything.

## Smoke suite (T1–T10)

| # | Scenario | Automated real-desktop coverage | Live (model) coverage |
|---|---|---|---|
| T1 | Open an app | `launches_are_verified_by_their_window` (Notepad, File Explorer, VS Code if installed) | scenario 1 |
| T2 | Type into an app | `notepad_focus_type_unicode_and_read_back`, `operator_tools_observe_act_and_verify_on_a_real_desktop` | scenario 2 |
| T3 | Multi-step file task | scripted e2e (file tools + verification) | scenario 3 |
| T4 | VS Code / developer task | VS Code launch check (skipped when not installed) | scenario 4 (write + run hello.py) |
| T5 | Browser task | `chromium_is_driven_through_devtools_on_the_desktop` | scenario 5 (example.com heading) |
| T6 | User takeover | `user_takeover_is_detected_and_switching_away_pauses` | manual |
| T7 | Stop mid-action | `stop_interrupts_typing_and_no_further_action_runs` | manual |
| T8 | Approval | scripted e2e (approval → overlay "Waiting for your OK", denial) | harness denies CRITICAL; report lists requests |
| T9 | Failure and recovery | expectation NotMet reported in the desktop operator test; scripted recovery e2e | scenario 9 (nonexistent button must not be reported done) |
| T10 | Restart mid-task | `a_crash_mid_action_leaves_a_coherent_history` (scripted; real DB) | manual |

## Results

### Real Windows desktop — PASS (9/9)

Environment: GitHub-hosted `windows-latest` (Windows Server), console session
of `runneradmin`, one 1024×768 display, Google Chrome preinstalled, VS Code
not installed, no human at the machine, no AI model. Run
`37495178940` (commit `a3fc244`), 35 s for the suite.

| Test | Result | Observed |
|---|---|---|
| screen_displays_and_capture | PASS | 1 display 1024×768; full capture in 102 ms |
| notepad_focus_type_unicode_and_read_back | PASS | 14 UIA elements; typed `IGRIS operator test — ünïcødé ✓ 日本` and read it back exactly |
| input_events_reset_the_idle_timer_and_focus_switches_between_apps | PASS | `SetCursorPos` does **not** reset the idle timer, a SendInput key does (→ 0 ms); focus switches Notepad ↔ console |
| operator_tools_observe_act_and_verify_on_a_real_desktop | PASS | observe with screenshot 365 ms; type verified by read-back; Ctrl+S with expectation "Save As open" → verified; Esc with "Save As gone" → verified; an unmet expectation is reported NOT met |
| user_takeover_is_detected_and_switching_away_pauses | PASS | user key press detected, observation marked stale; switching app → Paused "You switched to another window, so I paused." |
| stop_interrupts_typing_and_no_further_action_runs | PASS | stop during a 4000-char type → "Stopped by the user."; 288 chars had reached the field; nothing ran afterwards |
| launches_are_verified_by_their_window | PASS (VS Code skipped) | Notepad and File Explorer windows verified; File Explorer exposes 128 UIA elements; VS Code not installed |
| ocr_reads_text_from_the_screen | PASS | Windows.Media.Ocr: 63 lines in 912 ms; found `IGRIS OCR CHECK 4271` with its rectangle |
| chromium_is_driven_through_devtools_on_the_desktop | PASS | Chrome found; start + navigate 5.9 s; visible window; DOM click changed the page as expected |

### Real Chromium on Linux — PASS

`drives_a_real_chromium_through_devtools` against Chromium 1194 (headless) in
the development container.

### Live operator with a real provider — NOT RUN

No provider key was available in the development environment and the
`Operator live` workflow needs the `IGRIS_AI_API_KEY` secret, so the live scenarios have not
been run. An attempt (`Operator live` run `37496913264`, on `dfadba3`) stopped at its
"Provider key present" step because the secret isn't set. The harness and workflow are in place; until they run, **the
quality of a real model driving the desktop is unmeasured.**

### Manual on a real Windows 10/11 PC — NOT RUN

No one has yet run IGRIS by hand on a personal Windows machine with this phase.
Checklist for that run:

1. Ask "open Notepad and write a shopping list" — Notepad opens, text appears, the reply says it was verified.
2. While it types, move the mouse / press a key — it notices; switch to another app — it pauses and says so.
3. Press Stop in the overlay mid-task — it stops at once; the task card says Stopped.
4. Ask it to email or delete something — an approval appears; the overlay says "Waiting for your OK"; deny → nothing happens.
5. Ask for a VS Code task in a registered project — VS Code opens on the project; edits are made through file tools; tests run through the terminal allowlist.
6. Ask it to open a site and read the heading — IGRIS's own Chrome/Edge window opens (not your logged-in profile).
7. Kill IGRIS from Task Manager mid-task, start it again — the task shows as interrupted with "What happened", the reply says it was interrupted, nothing resumes on its own.
8. Ask it to click something that doesn't exist — it says it couldn't, does not claim success.

## Known gaps

- The runner is Windows Server without a person, so the overlay's appearance, DPI scaling above 100 %, multi-monitor setups and real user takeover timing are not covered.
- OCR needs a Windows OCR language pack (present on the runner and on most desktop installs).
- Browser control drives IGRIS's own browser profile, never the user's signed-in one.
