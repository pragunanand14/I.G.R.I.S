# IGRIS architecture

## Principles

1. **The backend owns privilege.** Secrets, the database, and all OS access live in Rust. The webview gets a
   minimal Tauri capability set (window controls only) plus IGRIS's own commands.
2. **No fabricated state.** Missing data is `null` end-to-end and rendered as `—`. Unbuilt features are labelled
   with their roadmap phase.
3. **Validate at the boundary.** Every command input is deserialized into a typed struct
   (`deny_unknown_fields` where applicable) and validated before it touches state.
4. **Modular by layer.** UI → services (typed IPC) → commands (thin) → domain modules → storage/OS.

## Runtime

```
React UI ──invoke──▶ commands/*  ──▶ settings / system / config / db
   ▲                                     │
   └──────── typed JSON (camelCase) ◀────┘
```

* `AppState` (managed by Tauri) holds config, the DB handle, the telemetry monitor, and the connectivity monitor.
* Errors cross IPC as `{ kind, message }` (`AppError`), normalised in the UI by `BackendError`.

## Persistence

SQLite (bundled via `rusqlite`) with WAL. Schema changes are forward-only migrations in
`src-tauri/src/db/migrations.rs`, tracked with `PRAGMA user_version`; each runs in its own transaction.
Tables are added in the phase that needs them (Phase 1: `settings`).

Settings are stored one JSON value per key, layered over typed defaults. Corrupt or obsolete values fall back to
defaults per field and are logged, so a bad row can't brick startup.

## Telemetry

`SystemMonitor` keeps `sysinfo` state between samples because CPU and network figures are deltas: the first CPU
sample is `null`, never `0`. Battery comes from `starship-battery` (`null` when absent). Connectivity is measured by
a background thread that TCP-connects to public anycast resolvers every 15 s (`unknown` until the first probe).
The UI polls at the user-configured interval and pauses while the window is hidden.

## Logging

`tracing` with an `event` field of stable UPPER_SNAKE names (`APP_STARTED`, `DB_MIGRATION_APPLIED`,
`SETTINGS_UPDATED`, `CONNECTIVITY_CHANGED`, …). Human-readable to stdout; JSON to a daily-rotated file in the app
log dir. Secrets are redacted (`AppConfig`'s `Debug` impl), and settings updates log field names, not values.

## Planned architecture (later phases)

* **AI layer** — `AiProvider` trait in Rust (Anthropic, OpenAI-compatible incl. local models, Gemini later),
  streaming to the UI over Tauri channels. Keys never leave the backend.
* **Orchestrator** — intent → memory retrieval → plan → permission check → tool execution → validation → response.
* **Tools** — each tool declares `name`, `description`, `inputSchema`, `permissionLevel`, `validate()`,
  `execute()`. Permission levels: SAFE, LOW, SENSITIVE (confirm), CRITICAL (always explicit confirm).
  No arbitrary shell execution: allowlists, path boundaries, and audit logging.
* **Untrusted content** — web pages, files, and tool output are data, never instructions, and are fenced as such
  in model context.
