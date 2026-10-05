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

## Web (Phase 5)

* **`web_search`** — one tool name, chosen per conversation when it is created (and frozen with it):
  a Brave or Tavily key (`SEARCH_PROVIDER` + `SEARCH_API_KEY`) gives the client tool in `tools/web.rs`; otherwise,
  with Anthropic as the AI provider, Anthropic's built-in server tool (`web_search_20260209`, `max_uses: 5`) is
  sent verbatim (`ToolDef.server`). OpenAI-compatible providers skip server tools. With neither, the tool isn't
  offered and the system prompt tells the model to say its information may be out of date.
* **Server tools** are executed by the provider: the Anthropic stream's `server_tool_use` /
  `web_search_tool_result` blocks surface as `StreamEvent::ServerTool` so the UI shows "Searching the web…" with
  result links, and each search is written to the audit log. `pause_turn` is handled by re-sending the paused
  assistant turn (no extra user message), counted against the 8-round limit. Error results (`error_code`) are shown
  as failures.
* **`fetch_url`** (SAFE) — http/https only, no credentials in URLs, no `localhost`/`.local`/`.internal`; the host
  is resolved and every address checked against loopback, private, link-local (incl. cloud metadata
  169.254.169.254), CGNAT, multicast, reserved and IPv6 ULA/link-local ranges. The connection is pinned to the
  checked address (`reqwest::ClientBuilder::resolve`) so DNS rebinding can't swap it, redirects are followed
  manually (max 5) with the same checks each hop, only text-like content types are accepted, downloads are capped at
  2 MB and text at 20k characters. HTML is converted to readable text.
* **Prompt-injection defense** — search results and pages are wrapped in `<untrusted_web_content>` with an explicit
  "data, not instructions" note; the system prompt repeats that rule.
* **Sources** — each tool activity carries `sources` (title + URL), rendered as links that open in the system
  browser.

## Voice (Phase 6)

```
mic button / Ctrl+Shift+Space / wake word
        │
  voiceStore.listen() ── stops speech + cancels a reply in progress (interruption)
        │
  STT: browser (Web Speech)  or  MediaRecorder → silence detector → transcribe_audio (Rust → /audio/transcriptions)
        │ text
  chat send (same path as typing) ── streamed reply ── SentenceChunker → Speaker (browser voices or synthesize_speech)
                                                         │ while speaking: mic level monitor → barge-in → listen()
```

* **Backends** (`voice/mod.rs`): `STT_PROVIDER` / `TTS_PROVIDER` = `browser` (default), `openai`, or `local`
  (OpenAI-compatible speech server at `VOICE_BASE_URL`). Keys stay in the backend; misconfiguration is reported as an
  actionable message, never a silent failure.
* **Recording**: `MediaRecorder` with an RMS-based `SilenceDetector` (ends 1.2 s after speech, gives up after 8 s of
  silence, caps at 30 s). Silence → "I didn't hear anything", nothing is sent.
* **Spoken replies**: only for requests made by voice (and only with *Speak replies* on). Markdown is converted to
  speakable text (code blocks are announced, not read; URLs become "the link"); sentences are spoken as soon as they
  complete in the stream.
* **Interruption**: Esc, the mic button, the hotkey, or talking over IGRIS (sustained mic level while speaking)
  stops speech, cancels the in-flight reply and starts listening.
* **Wake word** (experimental, off by default): continuous Web Speech recognition matched against "IGRIS" and common
  mishearings. Only offered when the webview provides speech recognition (WebView2 on Windows; not WebKitGTK).
* **Voice states** drive the core: listening, thinking (transcribing / generating), speaking, idle.
* **Not verified in CI**: real microphone capture and audio playback require hardware — logic is covered by unit
  tests and the request paths by mock-server tests.

## Computer control (Phase 7)

```
tool call (path) ─▶ files::guard(allowed_folders, path, Read|Write)
                      ├─ lexical normalise (rejects `..` escapes) → canonicalise the deepest existing ancestor
                      ├─ must sit inside a shared folder (symlinks resolved, so links out are refused)
                      ├─ Write access requires the folder to be marked "allow changes"
                      └─ credential/key files (.env, id_rsa, *.pem, *.key, credentials…) are always refused
```

* **Shared folders** (`files/`, table `allowed_folders`): file tools work only inside folders the user shares on the
  Tools page. Folders are read-only unless *Allow changes* is on. Drive roots, the home folder itself and system
  folders are rejected as too broad.
* **File tools** (`tools/files.rs`): `list_directory`, `search_files` (name match, skips build/VCS folders, capped),
  `read_file` (text only, 200 KB cap, wrapped as data), `create_file` / `create_folder` (LOW — never overwrite),
  `write_file`, `move_path`, `trash_path` (SENSITIVE — always ask; deletion goes to the Recycle Bin / Trash,
  permanent deletion does not exist), `open_path` (opens a document with its default app; executables and scripts
  are refused).
* **Processes** (`tools/processes.rs`): `list_processes` (top by CPU/memory, threads excluded), `close_application`
  (SENSITIVE; only apps on the user's allowlist, by name; sends a normal close request — SIGTERM / `taskkill`
  without `/F` — so the app can prompt to save), `open_url` (http/https only, default browser).
* **Projects** (`projects/`, table `projects`): user-registered projects with detected language/stack, repository and
  git branch (`detect_project` reads manifests such as `package.json`, `Cargo.toml`, `pom.xml`). `list_projects` and
  `get_project_context` (details, notes, branch, top-level files, README excerpt) give the model project context.
  Adding a project can share its folder read-only.
* **Not provided**: arbitrary shell commands, killing processes forcibly, permanent deletion, access outside shared
  folders.

## Productivity (Phase 8)

```
"remind me tomorrow at 5pm"  ─▶ set_reminder(text, when) ─▶ productivity::time::parse_when ─▶ reminders (SQLite)
                                                                (backend clock, local TZ)          │
scheduler (1 s tick, spawn_blocking) ─▶ take_due: UPDATE … SET status='fired' … RETURNING ──────────┘
        └─▶ OS notification (tauri-plugin-notification) + `reminder-fired` event ─▶ in-app alert (snooze / done)
```

* **Time parsing** (`productivity/time.rs`): the conversation's system prompt is frozen with only the date, so the
  model passes the user's phrasing ("in 20 minutes", "tomorrow at 5pm", "friday 9:30am", "20 october 6pm", ISO) and
  the backend resolves it against the real clock. Results always echo the resolved time; past times and unreadable
  phrases are errors with examples. `get_datetime` gives the model the exact current time.
* **Storage**: UTC `YYYY-MM-DDTHH:MM:SSZ` strings (ordered as text). Tables `tasks`, `reminders` (reminders and
  timers; `pending → fired → dismissed`, `cancelled`, snooze returns to `pending`) and `events`.
* **Scheduler** (`productivity/mod.rs`): fires each due item exactly once (atomic `UPDATE … RETURNING`). Items that
  came due while IGRIS was closed fire on the next start and are labelled *missed*. Reminders only fire while IGRIS is
  running.
* **Tools** (all local data): `get_datetime`, `list_tasks`, `list_reminders`, `list_events` (SAFE); `add_task`,
  `update_task`, `delete_task`, `set_reminder`, `start_timer`, `cancel_reminder`, `add_event`, `delete_event` (LOW).
* **UI**: Tasks page (tasks with natural-language due dates and live preview, reminders and timers with countdowns,
  a two-week calendar agenda) and a global alert stack with a synthesized chime.
* **Not provided**: calendar sync with Google/Outlook (needs OAuth app registration — TODO), recurring reminders,
  notifications for events or task due dates (set a reminder instead).

## Planned architecture (later phases)

* **Orchestrator** — intent analysis, memory retrieval and planning slot into `core/` ahead of the tool loop.
* **Multimodal** (Phase 9) — image/PDF input and screenshots; **hardening** (Phase 10) — security review, CI and the
  Windows installer.
* **Untrusted content** — web pages, files, and tool output are data, never instructions, and are fenced as such
  in model context.
