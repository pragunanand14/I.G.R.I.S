# IGRIS

**Intelligent General-purpose Responsive Intelligence System** — a personal AI operating layer for the desktop.

[![CI](https://github.com/pragunanand14/I.G.R.I.S/actions/workflows/ci.yml/badge.svg)](https://github.com/pragunanand14/I.G.R.I.S/actions/workflows/ci.yml)

IGRIS is a Tauri 2 desktop app: a React/TypeScript interface on top of a Rust backend that owns every privileged
capability (secrets, storage, OS access). The AI can only act through IGRIS's own validated, permission-checked and
audited tools — there is no shell access.

> IGRIS never pretends. Anything not built is labelled as such, unavailable data is shown as `—`, and the assistant
> only claims an action happened when a tool confirms it.

## What it can do

- **Chat** with Anthropic Claude, Google Gemini (free tier), OpenAI, or a local OpenAI-compatible model (Ollama, LM Studio, llama.cpp) —
  streaming, history, edit / regenerate / stop.
- **Memory** — remembers facts and preferences you ask it to, recalls relevant ones automatically; inspect, edit or
  delete everything on the Memory page. Refuses to store passwords, keys and ID numbers.
- **Web** — searches (Anthropic's built-in search, Brave or Tavily) and reads pages, with source links.
- **Voice** — push-to-talk (mic button or Ctrl+Shift+Space), natural spoken replies (Groq Orpheus or Gemini voices,
  or the system voice), talk over it to interrupt, and an
  optional always-on wake word ("IGRIS, open Spotify") with spoken yes/no approvals.
- **Your computer** — works with files only in folders you share (read-only unless you allow changes; overwriting,
  moving and deleting always ask first), opens and closes apps you allow, opens links and documents, shows running
  processes, loads context for registered projects.
- **Productivity** — tasks, reminders and timers from plain language ("remind me tomorrow at 5pm"), desktop
  notifications with snooze, a local calendar.
- **Images, PDFs and screenshots** — attach, paste or drop files into chat; ask IGRIS to look at your screen (it
  asks first, every time).
- **Operator mode** — "IGRIS, draft an email to Professor Sharma saying I'll submit tomorrow": IGRIS asks to take
  control, then operates your apps itself (Windows) — observing the screen, clicking, typing, checking the result —
  with a glowing screen border and a floating orb showing what it's doing. Esc (or Stop, or "IGRIS, stop") takes
  control back instantly. It prepares freely but asks before anything is sent, posted, bought or deleted.
- **Builds software** — writes code in your shared project folders and runs build/test/install commands (no shell,
  allowlisted tools, each command approved), reads the errors and fixes them; can open the project in VS Code.
- **Browser control** — in operator mode IGRIS opens its own Chrome/Edge window (separate profile) and works on the
  page's structure (links, buttons, fields) instead of pixels; buttons that send, pay or delete always ask first.
- **Tasks** — requests that take action ("create a project", "fix the failing tests") run as tasks: a plan when
  there are several steps, every action checked by IGRIS itself (a written file is read back, a command's exit code is
  read), a task card with progress, Pause / Resume / Stop, and an honest outcome — *done · verified*, *failed*, or
  *finished · not verified*. Interrupted tasks survive a restart and wait for you to resume them. Questions stay
  plain chat.
- **Security page** — choose which actions need your approval, and review the audit log of everything IGRIS did.

## Status

| Phase | Scope | State |
| --- | --- | --- |
| 1 — Foundation | App shell, theme, home screen + AI core, navigation, settings, real system telemetry, SQLite + migrations, structured logging | ✅ Done |
| 2 — AI chat | Provider abstraction (Anthropic, OpenAI, local OpenAI-compatible), streaming chat, conversation history, edit/regenerate/stop | ✅ Done |
| 3 — Tools | Tool registry + router, SAFE/LOW/SENSITIVE/CRITICAL permission layer with in-chat approvals, audit log, calculator, system info, allowlisted app launcher | ✅ Done |
| 4 — Memory | Long-term + knowledge memory (SQLite FTS5), automatic relevant-memory retrieval, remember / search / update / forget tools, sensitive-data guard, Memory page | ✅ Done |
| 5 — Web | `web_search` (Anthropic built-in, Brave or Tavily), `fetch_url` with SSRF protection, untrusted-content marking, source links | ✅ Done |
| 6 — Voice | Push-to-talk (mic button + Ctrl+Shift+Space), Whisper-compatible or built-in speech recognition, sentence-streamed spoken replies, barge-in interruption, experimental wake word | ✅ Done |
| 7 — Computer control | Shared folders (read-only or writable) with path-escape and credential-file protection, file tools (list, search, read, create, overwrite/move/trash with approval), process list, close allowlisted apps, open URLs/documents, project registry with stack detection | ✅ Done |
| 8 — Productivity | Tasks with due dates and priorities, reminders and timers from natural language ("tomorrow at 5pm"), desktop notifications with snooze, local calendar; all manageable from chat | ✅ Done |
| 9 — Multimodal | Attach images and PDFs (picker, paste, drag-and-drop), vision through Anthropic and OpenAI-compatible providers, PDF reading (native or extracted text), screenshot tool that always asks first | ✅ Done |
| 10 — Hardening | Security review ([docs/SECURITY.md](docs/SECURITY.md)), secret redaction in the audit log, crash-safe release builds, lazy-loaded UI, CI on Linux + Windows, Windows installer workflow | ✅ Done |
| 11 — Computer operator | Operator mode: Windows computer control (UI Automation, mouse, keyboard, windows, per-display capture), closed-loop observe → act → verify task engine with pause/stop/takeover handling, consequential-action confirmation, desktop overlay (border + orb), `run_command` for development, `copy_path` | ✅ Foundation |
| 12 — Orchestrator | Task model + state machine shared with operator mode, agent loop extracted from chat, plans, verification, bounded recovery, pause/resume/stop, restart-safe tasks, tool refresh, task card | ✅ v1 |
| 13 — Operator hardening | Real-Windows desktop test suite on CI, structured expected outcomes + change detection, window-verified launches, Chromium DevTools browser control, Windows OCR fallback, focused tool sets + `request_tools`, crash-safe task activity log, live-provider harness | ✅ v1 |
| 14 — Shared core | `igris-core` crate: the platform-independent IGRIS (no Tauri/OS code, CI-guarded); the app supplies device behaviour through seams | ✅ Done |
| 15 — Android | The same app on Android ([docs/ANDROID.md](docs/ANDROID.md)): phone tool set (apps, opening apps, battery/network) via a Kotlin plugin, phone-aware prompt and UI, emulator smoke test on CI | ✅ First slice |

**Not implemented** (and IGRIS says so if asked): operator mode on macOS/Linux, arbitrary shell commands (only
allowlisted developer tools via `run_command`), long-running background processes, calendar sync with Google/Outlook,
recurring reminders, screenshots on Linux Wayland sessions, Office documents, image generation, encryption of local
data at rest.

## Install (Windows)

Installers are built by the **Release** GitHub Actions workflow:

- Run it from **Actions → Release → Run workflow** and download `igris-windows-installers` from the run, or
- push a tag like `v0.1.0` to get a draft GitHub release with the `.exe` (NSIS) and `.msi` installers.

The installers are not code-signed yet, so Windows SmartScreen will warn on first run (*More info → Run anyway*).
WebView2 is installed automatically if missing.

## Develop

Prerequisites: Node 20+, Rust (stable), and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
for your OS (Windows: WebView2 + MSVC build tools).

```bash
npm install             # also generates app icons from assets/igris-logo.svg if missing
npm run tauri dev       # run the desktop app with hot reload
npm run tauri build     # build installers for the current OS
```

`npm run dev` alone serves the UI in a browser **without** a backend — native features then report themselves as
unavailable rather than showing fake data.

## Configure

IGRIS reads `.env` from the app config directory (`%APPDATA%\dev.igris.app\.env` on Windows) and from the working
directory; real environment variables win. Copy [`.env.example`](.env.example) and fill in what you need. For chat,
at least:

```ini
AI_PROVIDER=anthropic        # or openai, gemini, or local (Ollama / LM Studio / llama.cpp server)
AI_API_KEY=sk-ant-...        # not needed for local
# AI_MODEL=claude-opus-5-5   # default for anthropic; required for openai/local
# AI_BASE_URL=http://localhost:11434/v1   # local servers only
```

After editing `.env`, use **Settings → AI → Reload configuration** (no restart needed).

Optional: `AI_VISION_MODEL` (a model for messages with images, when the chat model can't see them), `AI_FAST_MODEL`
(a cheaper model for condensing long conversations) and `AI_CONTEXT_WINDOW` (the context size of your model, mainly
for local servers). All use the same provider as `AI_MODEL`; when unset, the chat model does everything.

**Gemini (free tier):** create a key at [aistudio.google.com/apikey](https://aistudio.google.com/apikey), then set
`AI_PROVIDER=gemini`, `AI_API_KEY=<key>` and `AI_MODEL=` a model listed in AI Studio (e.g. `gemini-2.5-flash`). The
free tier is rate-limited, and Google may use free-tier prompts to improve its products.

**Ollama:** `AI_PROVIDER=local` and `AI_MODEL=<name from ollama list>` are enough. Also set the environment variable
`OLLAMA_CONTEXT_LENGTH=16384` and restart Ollama — IGRIS's instructions and tool list (~7k tokens) don't fit Ollama's
default 4096-token context, and Ollama silently drops what doesn't fit. If you use a different context size, set
`AI_CONTEXT_WINDOW` to match so IGRIS budgets for it. Use a model with tool support; on a CPU,
small non-reasoning models answer much faster than reasoning ones (qwen3, deepseek-r1), whose thinking shows as
"Reasoning…" while they work. Secrets are read only by the
Rust backend and are never sent to the UI — the UI only learns whether a key is configured.

Then, as you need them:

- **Apps** — *Tools → Applications IGRIS may open*: *Find installed apps* (Start Menu + common locations) then
  *Add all*, or add apps one by one / *Browse…*.
- **Files** — *Tools → Shared folders*: share folders, or your whole user folder (secret folders such as `.ssh` and
  `AppData` stay blocked); turn on *Allow changes* only where IGRIS may create files.
- **Projects** — *Projects → Add project*: pick the folder; the stack is detected.
- **Operator mode** — nothing to set up; IGRIS asks when a task needs it. *Settings → Operator mode* changes the stop
  shortcut (Esc by default). For development tasks, share the project folder with *Allow changes* on.
- **Approvals** — *Security*: choose whether low-risk actions ask first. Sensitive actions always ask; for
  overwriting/moving files and closing apps you can choose *Allow for this chat* (until IGRIS restarts). Deleting and
  screenshots ask every time.
- **Wake word** — *Settings → Voice → Always listen for “IGRIS”*. Needs a speech service (`STT_PROVIDER` other than
  `browser`, e.g. Groq Whisper). IGRIS must be running (minimized is fine). Speech is detected locally, but each phrase
  heard while it is on is sent to the speech service to check for the name, which uses its quota.
- **Voice / web search** — see the voice and search sections of `.env.example`.

## Checks

```bash
npm run typecheck   # TypeScript
npm run lint        # ESLint
npm run test        # Vitest (UI, stores, services)
npm run test:rust   # cargo test (backend)
npm run check       # all of the above
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
```

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs all of these on Linux and Windows, plus
`npm audit` and `cargo audit`.

## Layout

```
src/                     React UI
  components/            chat, productivity, attachments, tools, core (AI orb), layout, ui primitives
  pages/ layouts/        routed screens (lazy-loaded) and the app shell
  stores/ hooks/         zustand state and side-effect hooks
  services/              the only code that talks to the backend (typed IPC), voice pipeline
  types/ utils/ config/
igris-core/src/          shared IGRIS core (no Tauri, no OS APIs) — see docs/ARCHITECTURE.md
  ai/                    provider trait, ModelRouter; Anthropic + OpenAI-compatible providers
  core/                  system prompt, context building, budgeting, compaction, the chat turn pipeline
  orchestrator/          tasks: state machine, agent loop, verification, recovery, persistence
  operator/ computer/    operator task engine; the computer Driver abstraction and safety policy
  tools/                 registry, schema validation, executor (permissions + audit), neutral tools
  memory/ files/ projects/ productivity/ attachments/ voice/
  conversations/ db/ settings/ config.rs error.rs
src-tauri/src/           the IGRIS app (Tauri) for desktop and Android
  commands/              thin IPC handlers — the entire UI-facing surface
  computer/              Windows driver (input, windows, UI Automation, OCR), Chromium DevTools control
  tools/                 desktop tools (apps, processes, terminal, files, screen capture, computer, browser)
  tools/phone.rs         Android phone tools (through plugins/tauri-plugin-igris-device)
  overlay.rs system/ state.rs logging.rs lib.rs
src-tauri/gen/android/   generated Android project
plugins/tauri-plugin-igris-device/   Android device capabilities (Kotlin)
docs/ARCHITECTURE.md     design, per phase
docs/ANDROID.md          the Android app: capabilities, build, limits, test results
docs/SECURITY.md         threat model, controls, review findings
```
