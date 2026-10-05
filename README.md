# IGRIS

**Intelligent General-purpose Responsive Intelligence System** — a personal AI operating layer for the desktop.

IGRIS is a Tauri 2 desktop app: a React/TypeScript interface on top of a Rust backend that owns every privileged
capability (secrets, storage, OS access). The interface can only reach the system through IGRIS's own typed,
validated commands.

> IGRIS never pretends. Anything not yet built is labelled as planned, and unavailable data is shown as `—`,
> not estimated.

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
| 8 — Productivity | Tasks with due dates and priorities, reminders and timers from natural language ("tomorrow at 5pm"), desktop notifications with snooze, local calendar; all manageable from chat. Calendar sync with Google/Outlook is not implemented | ✅ Done |
| 9–10 | Multimodal, hardening | Planned |

## Getting started

Prerequisites: Node 20+, Rust (stable), and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
for your OS (on Windows: WebView2 + MSVC build tools).

```bash
npm install           # also generates app icons from assets/igris-logo.svg
npm run tauri dev       # run the desktop app with hot reload
npm run tauri build     # produce an installer (NSIS/MSI on Windows)
```

`npm run dev` alone serves the UI in a browser **without** a backend — native features then report themselves as
unavailable rather than showing fake data.

### Configuration

Copy `.env.example` to `.env` and fill in what you need. To enable chat, set at least:

```ini
AI_PROVIDER=anthropic        # or openai, or local (Ollama / LM Studio / llama.cpp server)
AI_API_KEY=sk-ant-...        # not needed for local
# AI_MODEL=claude-opus-5-5   # default for anthropic; required for openai/local
# AI_BASE_URL=http://localhost:11434/v1   # local servers only
```

After editing `.env`, use **Settings → AI → Reload configuration** (no restart needed).

### Letting IGRIS open apps

Open **Tools → Applications IGRIS may open** and add apps (use *Find installed apps* or *Browse…*). IGRIS can
only open apps on that list, by name. To be asked before every launch, turn on **Security → Low → Ask first**. IGRIS reads `.env` from the app config directory
(`%APPDATA%\dev.igris.app\` on Windows) and from the working directory; real environment variables win. Secrets are
read only by the Rust backend and are never sent to the UI — the UI only learns whether a key is configured.

## Checks

```bash
npm run typecheck   # TypeScript
npm run lint        # ESLint
npm run test        # Vitest (UI, stores, services)
npm run test:rust   # cargo test (config, db, settings, telemetry)
npm run check       # all of the above
```

## Layout

```
src/                  React UI
  components/         core (AI orb), layout, system, ui primitives
  pages/ layouts/     routed screens and the app shell
  stores/ hooks/      zustand state and side-effect hooks
  services/           the only code that talks to the backend (typed IPC)
  types/ utils/ config/
src-tauri/src/        Rust backend
  commands/           thin IPC handlers (the entire UI-facing surface)
  ai/                 provider trait, Anthropic + OpenAI-compatible providers (incl. tool calling), SSE parser
  memory/             memory store (FTS5 search), retrieval, sensitive-data detection
  tools/              tool registry, schema validation, executor (permissions + audit), calculator,
                      system_info, application allowlist + launcher
  core/               orchestration: system prompt, context building, the chat turn pipeline
  conversations/      conversation + message persistence
  config.rs           env/.env loading, secret redaction
  db/                 SQLite connection + forward-only migrations
  settings/           typed settings, validation, persistence
  system/             telemetry (sysinfo), battery, connectivity
  logging.rs error.rs state.rs
docs/ARCHITECTURE.md  design and roadmap
```
