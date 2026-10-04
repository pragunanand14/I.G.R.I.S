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
| 3 — Tools | Tool registry, permission layer, calculator, system info, app launcher | Planned |
| 4 — Memory | Long-term + knowledge memory, management UI | Planned |
| 5–10 | Web, voice, computer control, productivity, multimodal, hardening | Planned |

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

After editing `.env`, use **Settings → AI → Reload configuration** (no restart needed). IGRIS reads `.env` from the app config directory
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
  ai/                 provider trait, Anthropic + OpenAI-compatible providers, SSE parser
  core/               orchestration: system prompt, context building, the chat turn pipeline
  conversations/      conversation + message persistence
  config.rs           env/.env loading, secret redaction
  db/                 SQLite connection + forward-only migrations
  settings/           typed settings, validation, persistence
  system/             telemetry (sysinfo), battery, connectivity
  logging.rs error.rs state.rs
docs/ARCHITECTURE.md  design and roadmap
```
