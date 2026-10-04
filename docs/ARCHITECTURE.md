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

## AI chat (Phase 2)

```
Composer ─chat_send(requestId, content, Channel)─▶ commands/chat
                                                     │ validate, resolve provider/model/effort
                                                     ▼
                         core/chat: save user msg → build context → provider.stream()
                                                     │ Delta events ──Channel──▶ UI (live text)
                                                     ▼
                         persist assistant msg with status → Finished event
```

* **Providers** implement `AiProvider` (`ai/mod.rs`): `AnthropicProvider` (Messages API over raw HTTP + SSE —
  there is no official Rust SDK) and `OpenAiCompatibleProvider` (OpenAI, or local servers such as Ollama).
  Gemini is not implemented and is reported as such. The provider is built from config at startup and can be
  rebuilt with `reload_config`.
* **Anthropic specifics**: default model `claude-opus-5-5`; thinking parameter omitted (adaptive by default);
  `output_config.effort` from Settings; automatic prompt caching; `fallbacks: "default"` (refusal fallback, beta
  `server-side-fallback-2026-07-01`) on the first-party endpoint for models that support it.
* **History integrity**: each conversation stores its system prompt at creation and never re-renders it;
  assistant turns store the provider's raw content blocks and replay them unchanged (thinking-block signatures
  bind to the exact prefix). Editing and regenerating only truncate from the tail. Failed / cancelled / refused
  turns are excluded from context, and raw blocks are never replayed to a different provider.
* **Outcomes are explicit**: every turn is persisted with a status — `complete`, `truncated`, `refused`,
  `cancelled` or `error` (with an actionable message). Nothing is shown as success unless the provider finished.
* **Cancellation**: each request has a client-generated id; `chat_cancel` trips a `CancellationToken` that aborts
  the HTTP stream. One generation per conversation at a time.
* **Retries**: 429 / 529 / 5xx / network errors are retried (max 3 attempts, honouring `retry-after`) only before
  any output has streamed.
* **Rendering**: Markdown via `react-markdown` without raw HTML (model output can't inject markup); links open in
  the system browser through the opener plugin (http/https only) rather than navigating the app window.
* **Limits / TODO**: no context-window management yet (very long conversations will eventually be rejected by the
  provider with a clear error); API keys come from `.env` only (OS keychain storage is a TODO).

## Planned architecture (later phases)

* **Orchestrator** — intent → memory retrieval → plan → permission check → tool execution → validation → response.
* **Tools** — each tool declares `name`, `description`, `inputSchema`, `permissionLevel`, `validate()`,
  `execute()`. Permission levels: SAFE, LOW, SENSITIVE (confirm), CRITICAL (always explicit confirm).
  No arbitrary shell execution: allowlists, path boundaries, and audit logging.
* **Untrusted content** — web pages, files, and tool output are data, never instructions, and are fenced as such
  in model context.
