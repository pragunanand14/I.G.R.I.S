# IGRIS architecture

## Principles

1. **The backend owns privilege.** Secrets, the database, and all OS access live in Rust. The webview gets a
   minimal Tauri capability set (window controls only) plus IGRIS's own commands.
2. **No fabricated state.** Missing data is `null` end-to-end and rendered as `—`. Unbuilt features are labelled
   with their roadmap phase.
3. **Validate at the boundary.** Every command input is deserialized into a typed struct
   (`deny_unknown_fields` where applicable) and validated before it touches state.
4. **Modular by layer.** UI → services (typed IPC) → commands (thin) → domain modules (the shared core) → storage/OS
   (through platform seams).

## Workspace: shared core and platform apps

IGRIS is a Cargo workspace (root `Cargo.toml`) with two crates:

| Crate | Path | What it is |
|---|---|---|
| `igris-core` | `igris-core/` | The platform-independent IGRIS core. No Tauri, webview, window system or OS API. |
| `igris` (lib `igris_lib`) | `src-tauri/` | The Windows/Tauri desktop app built on the core. |

The desktop crate re-exports the core's modules under their usual paths (`crate::ai`, `crate::orchestrator`, …;
`igris_lib::…` from its tests), and its `computer` and `tools` modules extend the core's with desktop parts, so
desktop code reads the same as before the split.

**In the core** (reusable by any IGRIS app): AI providers, capabilities and the ModelRouter; system prompt,
context building, budgeting and compaction, and the chat turn pipeline (`core/`); conversations; memory and
retrieval; attachments; the orchestrator (task model and state machine, agent loop, toolsets and focus,
verification, recovery, crash-safe persistence); the operator's task engine (pause, stop, takeover, verdicts);
the computer *abstraction* (the `Driver` trait, screen/UI types, state snapshots, expected outcomes, and the
safety policy: consequential names, messaging apps, risky key combinations); the tool framework (registry,
schema validation, permission levels, the executor with approvals and audit, redaction); the platform-neutral
tools (calculator, memory, productivity, projects, task, web, and the screenshot tool given a capturer);
productivity (tasks, reminders and their scheduler, events, time parsing); projects and allowed folders;
settings; voice backends (cloud STT/TTS); config; the database and its migrations; errors.

**In the desktop app**: Tauri setup and every IPC command; the overlay and global hotkeys; OS notifications;
the Windows driver (SendInput, UI Automation, window management, OCR) and `native()`; Chromium DevTools
browser control; desktop tools (applications, processes, terminal, files with the recycle bin, screen capture,
system info, computer and browser control) and the app's full tool registry (`tools/standard.rs`); system and
connectivity monitoring; logging; `AppState`.

**Seams** — how a platform app plugs device-specific behaviour into the core:

| Seam | Core side | Desktop supplies |
|---|---|---|
| Operating the screen | `computer::Driver` (`Operator::new(db, driver)`) | `computer::native()` → Windows driver (or `Unsupported`) |
| Platform tools | `tools::ToolRegistry::register`, `Tool` trait | `tools::standard::registry(Deps)` |
| Asking the user | `tools::executor::Approver` | approval round-trip through the UI |
| UI updates | `Orchestrator::on_event`, operator listeners, chat event callbacks | Tauri `emit` |
| Screen capture | `tools::screen::Capturer` | `tools::screen::screenshot_tool` (xcap / X11) |
| Reminder delivery | `productivity::run_scheduler(db, on_fire)` (a future) | spawned on Tauri's runtime; OS notification + event |
| Shortcut validity | `SettingsPatch::validate` checks the shape | `update_settings` checks the global-shortcut backend can register it |
| Opening paths/URLs | `open_path` / `open_url` callbacks in `Deps` | `tauri-plugin-opener` |

Rules: the core never depends on a platform crate (CI checks the core's dependency tree and lints it on its
own); a platform adds behaviour by implementing a seam, never by copying core logic; permissions, approvals
and the executor stay in the core so every platform gets the same security model. Test doubles (the fake
driver, the scripted AI provider) are behind the core's `test-support` feature for platform tests.

## Platforms: desktop and Android

`src-tauri` is one Tauri app built for both platforms; `#[cfg(desktop)]` /
`#[cfg(mobile)]` pick the platform parts at the seams above. On Android the
computer driver is `Unsupported` and operator mode is not offered; the device
layer is `plugins/tauri-plugin-igris-device` (Kotlin), reached only from IGRIS's
Rust tools (`tools/phone.rs`) so every call goes through the executor. The core
assembles the system prompt per `DeviceKind` (computer or phone) so IGRIS only
claims what the device can do; HTTPS on Android uses bundled roots
(`net::client_builder`). Details, limits and test results:
[ANDROID.md](ANDROID.md).

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
`igris-core/src/db/migrations.rs`, tracked with `PRAGMA user_version`; each runs in its own transaction.
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
  there is no official Rust SDK) and `OpenAiCompatibleProvider` (OpenAI, Gemini's OpenAI-compatible
  endpoint, or local servers such as Ollama). The provider is built from config at startup and can be
  rebuilt with `reload_config`.
* **Anthropic specifics**: default model `claude-opus-5-5`; thinking parameter omitted (adaptive by default);
  `output_config.effort` from Settings; automatic prompt caching; `fallbacks: "default"` (refusal fallback, beta
  `server-side-fallback-2026-07-01`) on the first-party endpoint for models that support it.
* **History integrity**: each conversation stores its system prompt at creation and never re-renders it;
  history is stored provider-neutrally (see *Intelligence interface* below), and a provider's own data (e.g.
  thinking-block signatures) is replayed unchanged only to that provider. Editing and regenerating only truncate
  from the tail. Failed / cancelled / refused turns are excluded from context.
* **Outcomes are explicit**: every turn is persisted with a status — `complete`, `truncated`, `refused`,
  `cancelled` or `error` (with an actionable message). Nothing is shown as success unless the provider finished.
* **Cancellation**: each request has a client-generated id; `chat_cancel` trips a `CancellationToken` that aborts
  the HTTP stream. One generation per conversation at a time.
* **Retries**: 429 / 529 / 5xx / network errors are retried (max 3 attempts, honouring `retry-after`) only before
  any output has streamed.
* **Rendering**: Markdown via `react-markdown` without raw HTML (model output can't inject markup); links open in
  the system browser through the opener plugin (http/https only) rather than navigating the app window.
* **Limits / TODO**: API keys come from `.env` only (OS keychain storage is a TODO).

## Intelligence interface

The layer between IGRIS and AI models. IGRIS asks for a *role*; history, budgets and capabilities are IGRIS
concepts; provider specifics stay inside the adapters.

```
core/chat ─▶ ModelRouter.route_for(role, needs) ─▶ Route { provider, model, capabilities }
    │
    ├─ provider-neutral history (core/context) ─▶ Budget for the route's model (core/budget)
    │        └─ over budget? ─▶ compaction (core/compaction): summary of older messages via the Fast role
    ▼
 AiProvider adapter (ai/anthropic, ai/openai) translates canonical turns to its wire format
```

* **Canonical history** (`ai/mod.rs`, `core/context.rs`): a `ChatTurn` holds text, tool calls, tool results, media
  and optional `ProviderExtras { provider, data }`. Extras are opaque data owned by one provider (Anthropic content
  blocks with thinking signatures, Gemini `thought_signature`s) and are replayed only by that provider's adapter;
  every other provider rebuilds the turn from the canonical fields. Older stored formats (a raw Anthropic block
  array, or turns with untagged `raw` / `provider_data`) are decoded on read and attributed to the provider that
  wrote them — nothing is rewritten in the database.
* **Adapters**: the Anthropic adapter sanitises and de-duplicates tool-call ids from other providers
  (its ids must match `^[a-zA-Z0-9_-]+$`); the OpenAI-compatible adapter (OpenAI, Gemini, local) echoes Gemini
  signatures only to Gemini. Switching provider mid-conversation keeps text, tool calls and tool results.
* **Model router** (`ai/router.rs`): roles `Chat` (`AI_MODEL`, or the Settings model), `Vision`
  (`AI_VISION_MODEL`) and `Fast` (`AI_FAST_MODEL`). Unset roles follow the chat model, so the default behaviour is
  one model for everything. A request with images goes to a model that can read them; if the chat model isn't
  known to read images and no vision model is configured, the request is still sent (logged as `MODEL_ROUTE_FALLBACK`) and the
  provider's answer decides. All roles use the configured provider.
* **Capabilities** (`ai/capabilities.rs`): a small table of model families (context window, tools, vision,
  reasoning) with conservative fallbacks for unknown models (32k, no vision; local servers 16k).
  `AI_CONTEXT_WINDOW` overrides the window.
* **Budget** (`core/budget.rs`): provider-neutral, deterministic estimate (≈4 ASCII characters per token, denser
  for other scripts, fixed costs for images and PDFs). The window is capped at 128k unless `AI_CONTEXT_WINDOW` is
  set; output room (up to 16k) and a 5% margin are reserved. Within a tool loop, older tool outputs are shortened
  progressively in the request copy only (the database keeps them in full); the newest results stay intact.
* **Compaction** (`core/compaction.rs`): only when the history doesn't fit. Older messages (split at user messages;
  the latest exchange is always kept verbatim) are summarised by the Fast role with no tools, in chunks, with
  instructions to keep goals, decisions, facts, open tasks and the state of active work. The summary is stored in
  `conversation_summaries` with the last message it covers and reused by later requests, then extended when needed.
  Memory attached to summarised messages is carried over verbatim. If summarising fails, an extractive digest is
  used for that request (not stored) and the reply still goes ahead. Editing a message deletes summaries that
  cover it. All original messages stay in the database and in the UI; the UI shows "Condensing earlier messages…".
* **Response depth** (Settings → AI): `low` / `medium` / `high`. Anthropic: `output_config.effort`. OpenAI
  reasoning models and Gemini 2.5+/3: `reasoning_effort` for low/high; medium leaves the provider default. Other
  models ignore it.

## Tools and permissions (Phase 3)

```
model ──tool_use──▶ agent loop ──────▶ tools/executor
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

## Multimodal (Phase 9)

```
Composer (pick / paste / drop) ─▶ attach_file (raw bytes IPC) ─▶ sniff by content, limits, PDF text extraction
        │ staged id                                              └─▶ <data>/attachments/<uuid>.<ext> + row (staged)
chat_send(content, attachmentIds) ─▶ link to the user message
generate ─▶ build_turns (Media refs) ─▶ hydrate from disk ─▶ Anthropic: image / document blocks
                                                            OpenAI-compatible: image_url data URLs; PDFs as extracted text
take_screenshot (SENSITIVE) ─▶ capture ─▶ ≤1568 px JPEG ─▶ attachment ─▶ tool_result with image
```

* **Attachments** (`attachments/`, table `attachments`): PNG, JPEG, GIF, WebP (≤ 5 MB, ≤ 8000 px) and PDF (≤ 20 MB),
  up to 5 per message. The type is sniffed from the bytes, never trusted from the name. Uploads are *staged* until
  sent; unsent ones are removed after 24 h. Rows cascade with their conversation/message and a GC removes orphaned
  files (on start and after deleting a conversation).
* **History integrity**: only references are persisted in messages and raw turns; bytes are re-read from disk for
  every request, so replayed history is byte-identical. A missing file becomes an explicit "no longer available" note.
* **Providers**: Anthropic gets native `image` and `document` blocks (PDF pages are seen visually). OpenAI-compatible
  providers get images as data URLs and PDFs as extracted text wrapped as untrusted `<attached_document>`; scanned
  PDFs without text are reported as unreadable. Image results from tools follow the tool message as user content.
  A model that rejects images gets an explanatory error.
* **Screenshots** (`tools/screen.rs`): SENSITIVE — always asks, and the approval says the image goes to the AI
  provider. Windows/macOS use `xcap`; Linux captures the X11 root window (Wayland sessions report that it's not
  supported). The capture is stored in the conversation and shown as a thumbnail.
* **Prompt injection**: text inside images, documents and screenshots is content, not instructions (system prompt).
* **Not provided**: Office documents (Word/Excel), audio/video files, image generation, OCR for providers without
  vision.

## Hardening (Phase 10)

* **Security review** — see [SECURITY.md](SECURITY.md) for the threat model, controls and findings. Fixes: secrets
  are redacted from the tool audit log; release builds unwind on panic so panic-contained parsers (PDF) can't crash
  the app.
* **Performance** — pages are lazy-loaded (main bundle 816 KB → 379 KB, 120 KB gzipped); the chat page with
  Markdown/highlighting loads on first visit. Release profile: LTO, one codegen unit, `opt-level = "s"`, stripped.
* **Consistency** — `rustfmt.toml` (wide lines, matching the codebase) enforced in CI; clippy with `-D warnings`.
* **CI** (`.github/workflows/ci.yml`) — frontend typecheck/lint/test/build and `npm audit`; Rust fmt, clippy and tests
  on Ubuntu and Windows; `cargo audit`.
* **Release** (`.github/workflows/release.yml`) — Windows NSIS + MSI installers as artifacts, or a draft GitHub
  release when a `v*` tag is pushed. Installers are not code-signed yet.

## Operator mode (computer operator)

IGRIS can operate the computer itself — see the screen, use mouse and keyboard, work in other apps — as a native
capability on the existing tool layer, not a separate bot.

```
chat turn ─ operator_start (SENSITIVE: user approves the task) ─┐
            ┌───────────────────────────────────────────────────┘
            ▼
   computer_observe ──► one action ──► "what changed" ──► observe again (verify) ──► … ──► operator_finish
   (window, a11y          click / type / key / scroll /                                    ("completed" only
    elements, optional    drag / focus window                                              after a fresh look)
    screenshot)
```

* **`computer/`** — the `Driver` trait (displays, windows, foreground, focus, capture, UI Automation elements,
  element at a point, mouse, keyboard, idle time). `windows.rs` implements it with Win32 `SendInput`,
  `SetForegroundWindow` (with the thread-input workaround), DWM bounds, UI Automation (`FindAllBuildCache` over
  interactive control types) and xcap capture per display. Other platforms get `Unsupported`, which says so. A fake
  desktop backs the tests. Targeting prefers accessibility elements (name, role, bounds) over screenshot coordinates;
  coordinates are mapped from the downscaled screenshot back to the display.
* **`operator/`** — the operator session. It uses the shared task state machine (`orchestrator/task.rs`, see
  *Orchestrator* below) and writes its own columns (state, steps, retries, result, error) into the row of the
  orchestrator task it belongs to in `operator_tasks`. One task at a time; it belongs to the chat turn that started it and always ends with that turn
  (control returns to the user). `checkpoint()` runs before every action: it enforces stop/pause, and when the user
  has used the mouse or keyboard since IGRIS's last input it requires a fresh observation — or pauses if they switched
  windows. Limits: 8 failed actions, 150 actions per task, 5 minutes paused.
* **`tools/computer.rs`** — `operator_start/update/finish`, `computer_observe`, `computer_click/type/key/scroll/drag/
  focus_window`, `computer_confirmed_action`. Actions refuse to run on a changed screen (different foreground window,
  or the element no longer at its observed position). Controls named Send/Post/Publish/Buy/Pay/Delete…, Ctrl+Enter,
  Alt+S, and Enter in chat apps are refused and must go through `computer_confirmed_action` (CRITICAL, always asks;
  its approval text is built from the element actually on screen). Typing into terminals and Win+R/Win+X are refused:
  commands go through `run_command`.
* **`tools/terminal.rs`** — `run_command`: an allowlisted developer tool + argument array, run without a shell in a
  writable shared folder, with secrets removed from the environment, a time limit, captured (clipped) output, and a
  Windows job object so timeouts kill the whole process tree. Routine build/test commands can be trusted for a chat;
  installs and everything else ask each time; publishing (`git push`, `npm publish`…) always asks; destructive git
  commands, credential/config changes and inline code (`python -c`, `node -e`) are refused.
* **Executor** — tools declare `operator_scoped()` (covered by the running task's approval instead of per-action
  prompts, audited as `operator`), `always_ask(input)` (forces a prompt even in an approved task or trusted chat) and
  `timeout(input)`.
* **Reliability** — `computer_observe` reads up to 4000 controls and shows 150, always including the focused control and
  every text field (so a form at the bottom of a busy page like Gmail's compose box is never cut off), with a `find`
  filter. Targets are re-checked by position (the control found there now must be the same control, its content or an
  unnamed container — not a popup over it). Typing refuses when focus isn't a text field (web apps read stray keys as
  shortcuts) and reads the field back afterwards. Safety refusals don't count toward the failure limit. Rate limits
  are retried after the delay the provider asks for (up to 60 s); a used-up daily quota is reported as such.
* **Agent loop** — up to 80 tool rounds while operator mode runs (30 for other tasks, 8 for plain chat). The
  operator-session tools (`computer_*`, `operator_update`, `operator_finish`) are only sent to the model while operator
  mode is running. Only the two most recent operator screenshots
  stay in the request; screenshots are never written to disk or the database.
* **Overlay (`overlay.rs`, `components/operator/`)** — two always-on-top, transparent, content-protected (excluded
  from capture) windows created on first use: a click-through animated border on the display IGRIS works on, and a
  draggable orb with the status line and Pause/Resume/Stop. They follow `operator-state` events, fade out ~2 s after
  the task ends, and the orb moves out of the way of IGRIS's own clicks. A global stop shortcut (Esc by default,
  configurable) is registered only while a task runs and briefly released when IGRIS itself presses Esc. The main
  window shows a banner with the same controls; the IGRIS core shows planning / executing / waiting / verifying /
  success states. Voice: "IGRIS, stop / pause / continue" control a running task.

## Operator hardening (real Windows)

How operator mode observes, acts and checks on a real desktop. The real-desktop test suite and results are in
[OPERATOR_TESTING.md](OPERATOR_TESTING.md).

```
observe (UIA controls · focused field · windows · optional screenshot · OCR fallback)
   → act on a semantic target (control index / DOM ref) with a stated expectation
   → state after the action (cheap snapshot, polled up to 2.5 s) → verdict: met / not met / unknown
   → orchestrator verification → recover (re-observe, adjust) or continue
```

* **Targeting order**: IGRIS's browser via DevTools DOM refs → UI Automation controls (indexes from
  `computer_observe`) → screenshot vision → OCR text positions → raw coordinates. Element targets are re-checked at
  their position before acting; coordinates need a screenshot.
* **Screen state** (`computer/state.rs`): a cheap snapshot from window and accessibility data (no screenshot): active
  window, open windows, focused control, focused field text (never password fields). Used before/after actions for
  change detection, in failure recovery ("IGRIS looked again — …"), and as one line in the task brief each round while
  operating.
* **Expected outcomes**: `computer_click` and `computer_key` take `expect` (`window_present`, `window_gone`,
  `element_present`, `element_gone`, `field_contains`, `focus_on`, `screen_changed`, or `none`). After the action the
  state is polled until it's met or 2.5 s pass. With `none`, IGRIS still reports whether anything visibly changed
  ("Nothing visibly changed — the action may not have worked"). `computer_type` reads the field back. Verdicts feed the
  task's verification: met → verified, not met → failed check (recovering), unknown → unverified.
* **Preconditions**: actions refuse when the window in front isn't the one observed (including shortcuts, which would
  go to the wrong app); element targets must still be at their observed position; typing needs a text field in focus;
  commands only run in shared folders (`run_command`).
* **Launch verification**: a launched app counts as verified only when one of its windows appears (polled up to 8 s;
  "VS Code" matches `Code.exe` / "Visual Studio Code"). A running process is accepted only where windows can't be
  listed at all.
* **Browser** (`computer/cdp.rs`, `computer/browser.rs`, `tools/browser.rs`): IGRIS starts its own Chrome/Edge with a
  separate profile and a DevTools port on 127.0.0.1 (never the user's profile; non-local endpoints are refused), and
  works on the DOM: `browser_open` (http/https only, waits for load), `browser_snapshot` (interactive elements with
  refs, field values except passwords, visible text — all marked untrusted), `browser_click` / `browser_type` (trusted
  in-page events — the user's mouse and keyboard aren't used; typed text is read back), `browser_confirmed_click`
  (CRITICAL, always asks; the prompt names the element actually on the page). Buttons that look like send / pay /
  delete are refused by `browser_click`. These are operator-session tools: they need an approved operator task. A
  minimal WebSocket client is built in (no new dependency).
* **OCR** (`Driver::ocr`, Windows.Media.Ocr): only when `computer_observe` is called with `ocr: true`, or
  automatically when a screenshot finds fewer than 3 readable controls. Lines come back with positions in screenshot
  coordinates. On systems without it, observation says "OCR unavailable" rather than guessing.
* **Focused tool sets** (`orchestrator/toolset.rs`): each request starts with a core set (calculation, system info,
  web, memory, reading files, apps and links, `operator_start`, `task_plan`, `request_tools`) plus the groups its
  words call for (productivity, file changes, terminal/projects, screen) and the groups the recent conversation used.
  `request_tools` adds groups for the rest of the turn. Hidden ≠ forbidden ≠ permitted: the executor's validation,
  permissions, approvals and audit apply to every call as before.
* **Overlay truth**: while an action waits for approval during an operator task, the orb and border show *waiting*;
  stop, pause and completion are mirrored from the task.
* **Crash persistence**: every task event (tool requested / started / completed, checks, approvals, state changes) is
  written to `task_events` as it happens (migration 11; ≤200 per task; details clipped and redacted). After a crash, an
  action that started but never finished is recorded with an *unknown* result, and the reply that never came is
  replaced by a message saying IGRIS closed and what had happened. The task card's "What happened" shows the log.
* **Live harness** (`tests/operator_live.rs`, workflow *Operator live*): the full app stack with a real provider on a
  real Windows desktop, scenario by scenario, writing `target/operator-live-report.md`. Its approver stands in for
  the user: it approves ordinary approvals and denies every CRITICAL action.

## Orchestrator (tasks)

Actionable requests become tasks with a lifecycle; conversation stays lightweight.

```
chat turn ─▶ core/chat: save message, Phase 2 context (router, budget, compaction), persist reply
                │
                ▼
     orchestrator/agent_loop ── model round ─▶ tool calls ─▶ tools/executor (validation, permission,
                │                                            approval, run, audit) — the only execution path
                │  first action / task_plan ─▶ task (intent.rs) ─▶ TaskSession (session.rs)
                ▼
     before a call: pause checkpoint, refusals · during: waiting_for_approval · after: verify ─▶ recover
                ▼
     end of turn: completed only with evidence · failed · ended (not verified) · cancelled
```

* **Chat or task** (`intent.rs`): every turn starts as chat — no task, no extra model call. It becomes a task when
  the model asks for an action that changes the computer (file changes, apps/links/documents, `run_command`,
  `operator_start`) or declares a plan with `task_plan` (SAFE). Questions, look-ups and IGRIS's own records (memory,
  reminders) stay chat.
* **Task model** (`task.rs`): id, conversation, kind (`general` / `operator`), objective (the user's words), state,
  plan (steps with pending/active/completed/failed/skipped), current step, live activity, action count, failure
  count, result/error, pause reason, project, and a bounded context (the last 24 actions with their verification).
  Stored in `operator_tasks` (generalized by migration 10; old operator rows and plain-string plans still load).
* **State machine**: `created, planning, waiting_for_approval, executing, paused, verifying, recovering, completed,
  failed, cancelled, ended`. Every change goes through `TaskState::advance`; illegal transitions are refused (e.g.
  completion only from `verifying`; `failed` reopens only when the user retries). Operator mode uses the same machine.
* **Hub** (`mod.rs`): running tasks, transitions (each persisted and sent to the UI as a `task-update` event), and
  pause / resume / stop. A task runs inside the chat turn that started it, but its state lives here and in the
  database, independent of the response stream. Operator pause, takeover and Esc/orb/voice stop are mirrored into the
  task.
* **Task context** (`brief.rs`): each round, a short `<task_state>` (plan, recent actions and checks, failures,
  notes such as "resumed") is added to the request copy only — never to the stored transcript. Phase 2 budgeting
  and compaction apply as for any turn.
* **Verification** (`verify.rs`): after a successful action — a read-only probe through the executor (`read_file`
  after a write, the folder listing after a move or delete), evidence the tool observed (exit code, process running
  after a launch), the open windows, or operator mode's own observe-then-finish rule. Otherwise the action is
  *unverified*. The verdict is added to the tool result (`[IGRIS check: …]`).
* **Recovery** (`recovery.rs`): failures are classified from the executor outcome. Only read-only calls are retried
  automatically, once; actions are never repeated automatically; an identical call that failed twice is refused;
  5 failed actions end the task; a denied or unanswered approval fails it; cancellation ends it.
* **Completion**: when the reply ends, the latest attempt at each target decides. Any failed or unconfirmed one →
  `failed`. Open plan steps, no actions, or unverifiable actions → `ended` with a note saying so. Only fully
  checked work is `completed`.
* **Pause / resume / stop**: the task card (and, for operator tasks, the orb, Esc and voice) pause, resume or stop.
  A pause holds the next round and any action already requested. After a resume, IGRIS re-checks earlier verified
  work itself, tells the model what changed, and an action decided before the pause is handed back instead of run.
  Stop cancels the running request and tool. 5 minutes paused cancels the task.
* **Restart**: tasks still running when IGRIS closed load as paused + interrupted and are never resumed
  automatically. *Resume* (or *Try again* for a failed task) records a message in the conversation and runs a new
  turn that re-checks first. Operator tasks must ask for operator mode again.
* **Tool refresh** (`toolset.rs`): tools belong to capability groups. A conversation's tool snapshot is re-evaluated
  against the registry and configuration when a task starts or resumes (new tools appear, removed ones go, no
  duplicates); conversations created without tools stay without tools.
* **Projects**: an action inside a registered project's folder links the task to that project.
* **Limits**: 30 rounds per task run (80 while operating the computer), 30 minutes per run, 5 failed actions, tool
  timeouts, cancellation at every step.

## Cross-device (phone ↔ PC)

`igris-core/src/device/` adds one user's other devices without a second brain:
a device identity (Ed25519 + X25519, sealed by a platform `KeyProtector`), a
trusted registry, code-and-confirmation pairing, end-to-end encrypted signed
envelopes, and a hub that routes them. A task from another device becomes an
ordinary conversation on the executing device, run by the app's `TaskRunner`
through `chat::generate` → orchestrator → executor with that device's own
policy; orchestrator events become ordered `task_update`s; pause/resume/stop
use `Orchestrator::control`; approvals race the local UI against a signed
remote approval verified by the executor. Memories sync with revisions and
tombstones. The separate `igris-relay` crate only authenticates and routes.
Details, protocol and limits: [CROSS_DEVICE.md](CROSS_DEVICE.md).

## Possible next steps

* Operator mode on macOS/Linux; browser automation via the DevTools protocol for pages with poor accessibility data;
  visual diffing between observations.

* Code signing for Windows installers; auto-update.
* Calendar sync (Google / Microsoft Graph) via OAuth; recurring reminders.
* Encryption of local data at rest; Wayland screenshots via the desktop portal.
