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

## Tools and permissions (Phase 3)

```
model ──tool_use──▶ core/chat loop ──▶ tools/executor
                                         1. malformed JSON?        → INVALID_JSON error result
                                         2. unknown / not offered? → "Unknown tool" error result
                                         3. schema + tool validate → "Invalid input" error result
                                         4. permission policy      → Approver (UI card) when required
                                         5. execute (30 s timeout, cancellable)
                                         6. audit log row (always)
                    ◀──tool_result────── result (is_error when anything failed)
```

* **Tool interface** (`tools/mod.rs`): `spec()` (name, title, description, JSON input schema, `PermissionLevel`),
  `describe(input)` (human summary for UI/approval), `validate(input)`, `execute(input)`. Schemas must be
  strict-compatible (asserted at registration) and are sent with `strict: true`, but IGRIS validates every input
  itself regardless (`tools/schema.rs`).
* **Permission levels**: SAFE never asks; LOW runs automatically unless *Ask first* is enabled; SENSITIVE always
  asks; CRITICAL always asks and can never be made automatic. Approvals arrive through `respond_tool_approval`,
  expire after 5 minutes (treated as "not performed") and are cancelled by Stop. Denials are reported to the
  model as errors so it can't claim success.
* **Tools**: `calculator` (recursive-descent parser — no code evaluation), `system_info` (real telemetry),
  `list_applications`, `launch_application` (LOW). Apps come from a user-managed allowlist (`applications`
  table); the model supplies only a name. Paths must be absolute, exist, and be executables (`.exe` only on
  Windows; scripts and shortcuts are rejected). Launches are verified: still running after 1.5 s, or exited 0
  (hand-off to a running instance); a non-zero exit is reported as a failure.
* **Agent loop** (`core/chat.rs`): up to 8 model ↔ tool rounds per message; tools only run when the model stopped
  with `tool_use` (never on `max_tokens` / `refusal`). The full exchange is stored as provider turns in the
  message's `raw` (`{"v":2,"turns":[...]}`) and replayed unchanged; tool activity (with text offsets for
  interleaved display) is stored in `messages.tool_activity`.
* **Frozen tool set**: each conversation stores the tool definitions it started with (`conversations.tool_specs`)
  and always sends exactly those — providers bind cached prefixes and reasoning to the tool list. Conversations
  created before Phase 3 have no snapshot and keep working without tools. Calls to tools outside the snapshot are
  rejected.
* **Audit log** (`tool_audit`): append-only, survives conversation deletion; records actor (IGRIS or you), tool,
  level, description, input, approval, outcome, result summary and duration. Shown on the Security page.
* **Local models**: if an OpenAI-compatible local server rejects tool definitions, IGRIS retries the turn as
  plain chat.

## Memory (Phase 4)

Three levels, as specified:

* **Conversation memory** — the conversation history itself (`conversations`, `messages`).
* **Long-term memory** — durable facts about the user (`memories.kind = 'long_term'`).
* **Knowledge memory** — information stored on purpose for later retrieval (`kind = 'knowledge'`).

Storage is a `memories` table with an FTS5 index (`porter unicode61` tokenizer) kept in sync by triggers. Every
memory records its source (you or IGRIS), timestamps and how often it was used, and is searchable, editable and
deletable on the Memory page.

**Retrieval.** When a user message is saved, `memory::retrieval::select` attaches long-term facts not yet sent
in this conversation (each *version* once) plus up to five knowledge items matching the message (BM25 ranking).
The rendered `<memory>` block — labelled as data, not instructions — is stored with the message
(`messages.memory_context`) and replayed byte-for-byte on later turns; editing or deleting a memory never rewrites
history (an edited fact is attached again as a new version). The chat shows "N memories used" on each message.

**Tools.** `remember` (LOW), `search_memory` (SAFE), `update_memory` (LOW), `forget_memory` (LOW) go through the
normal executor (validation, permission policy, audit). The system prompt tells the model to save only when asked
or for clearly lasting facts, and to forget via search → delete.

**Safety.** `memory::sensitive` refuses passwords/PINs, API keys and tokens, private keys, payment card numbers
(Luhn-checked) and government ID numbers (SSN, Aadhaar, PAN) — from the model and from the Memory page alike.
Memory content never appears in logs. **Settings → Use memory** (on the Memory page) turns retrieval and all memory
tools off without deleting anything. Duplicates are detected and not stored twice.

## Planned architecture (later phases)

* **Orchestrator** — intent analysis, memory retrieval and planning slot into `core/` ahead of the tool loop.
* **More tools** — web search (Phase 5), filesystem with path boundaries and process inspection (Phase 7),
  each declaring its permission level; destructive operations will be SENSITIVE/CRITICAL.
* **Untrusted content** — web pages, files, and tool output are data, never instructions, and are fenced as such
  in model context.
