# Cross-device IGRIS (phone ↔ PC)

One user, several trusted devices, one IGRIS. From the phone you can ask the
PC to do something ("open Notepad on my PC"), approve the PC's actions from the
phone, pause or stop them, and keep memories in sync. This is **not** a
multi-user service: there are no accounts, no registration and no cloud brain.

## What exists

| Piece | Where | What it does |
|---|---|---|
| Device identity | `igris-core/src/device/identity.rs` | Ed25519 signing key + X25519 key per installation. Device id = `dev_` + first 16 bytes of SHA-256(signing key). Private keys sealed at rest by the OS. |
| Trusted registry | `device/registry.rs` | The user's other devices (public keys only), trusted after explicit pairing; revocation is kept so a removed key can't quietly return. |
| Pairing | `device/pairing.rs` | One-time code (80-bit secret, 10 minutes, 5 attempts) + user confirmation on the inviting device. |
| Envelope | `device/envelope.rs` | End-to-end encryption, signature, replay protection for every message. |
| Signed approvals | `device/approval.rs` | Approving on one device an action that runs on another. |
| Protocol | `device/protocol.rs` | The typed messages (no "run a tool" / "run a command" message exists). |
| ntfy transport (default) | `device/ntfy.rs` | Carries the encrypted envelopes over a public [ntfy](https://ntfy.sh) service (ntfy.sh by default, or your own ntfy server): nothing to set up, only the pairing code. |
| Relay link (optional) | `device/link.rs` | Outbound authenticated WebSocket to your own `igris-relay`, reconnect with backoff. |
| Hub | `device/hub.rs` | Routing: remote tasks through the existing orchestrator, control, approvals, presence, pairing, memory sync. |
| Memory sync | `device/sync.rs` | Revisions + tombstones; memories only. |
| Targeting | `device/target.rs` | "on my laptop", "here", "my phone", names; asks when ambiguous. |
| Tools | `igris-core/src/tools/devices.rs` | `list_devices` (SAFE), `send_to_device` (LOW), `device_task_status` (SAFE), `device_task_control` (LOW). |
| Relay | `igris-relay/` | Optional separate minimal server binary, for people who want to run their own. |
| App | `src-tauri/src/devices.rs`, `commands/devices.rs` | Key protectors (Windows DPAPI, Android Keystore), task runner, commands, notifications. |
| UI | Settings → Devices (desktop), More → Your devices (phone), prompts on every screen | Connect, pair, remove, send tasks, approve, see status. |

## Trust model

* **Identity is a key**, never a hostname, IP, MAC or session id.
* **Trust is explicit.** A device becomes trusted only when the user enters a
  one-time code shown on an already-trusted device *and* the user of that
  device confirms "Allow Pixel (Android phone) to join your IGRIS?". Being on
  the same network means nothing.
* **Owner.** Each installation starts with its own owner id; a joining device
  adopts the inviter's. Messages from a device with another owner id are refused.
* **Authorization is separate from authentication.** Being trusted lets a
  device *ask*. What runs is decided by the executing device's own tools,
  permission policy and approvals. Specifically:
  * any trusted device may request a task, and pause/resume/stop or approve
    actions of tasks *it* requested — nothing else;
  * only computers (Windows/Linux/macOS) may announce new devices or remove
    other devices for the whole IGRIS; a phone can only remove devices from
    its own registry (or itself);
  * there are no remote settings, no remote shell, no arbitrary RPC.
* **Private keys never leave the device** and are never sent to ntfy or the relay.
  At rest: DPAPI (Windows, sealed to the user account), Android Keystore
  (non-exportable AES-GCM key), or — on Linux/macOS builds — stored unsealed
  in the app database (reported as such in the UI).

## Protocol

### Transport

The transport only moves sealed envelopes; it is not trusted with anything
(see the envelope below). Which one is used is the "server address" in
Settings → Devices → Advanced:

| Address | Transport |
|---|---|
| empty (default) | ntfy over `https://ntfy.sh` |
| `https://…` (or `http://` to this machine) | ntfy over your own ntfy server |
| `wss://…` (or `ws://` to this machine) | your own `igris-relay` |

Cross-device turns on by itself when you show or type a pairing code.

### ntfy (default)

[ntfy](https://docs.ntfy.sh) is a free, open-source publish/subscribe service:
anyone can post to a "topic" over HTTPS and anyone who knows the topic name
can read it. IGRIS uses it as a mailbox:

* **Inbox** of each device: topic `igris_` + hex(SHA-256(`igris-ntfy-inbox-v1`,
  owner id, device id))[..16 bytes]. The owner id is a random 128-bit value
  that only the user's devices know (it is exchanged inside the encrypted
  pairing), so outsiders can't compute or find the inbox.
* **Pairing** meets at a topic derived from the code's pairing id; the code
  itself (and its secret) is never sent.
* A device subscribes to its inbox (`GET /<topic>/json`, a long-lived HTTPS
  stream), catching up with the messages of the last 15 minutes after a
  restart, and publishes with `POST /<topic>` (`X-Firebase: no`).
* Every post is JSON: an envelope `{"k":"e", "f", "to", "env"}`, presence
  `hi`/`ho` (on connect and every 45 minutes; a peer is shown offline after
  100 minutes without one), or pairing join/accept blobs. Posts over 3.5 KB
  (large memory-sync batches) are split into parts and reassembled.
* Everything that matters is inside the envelope (encrypted and signed); a
  forged, replayed or garbled post is simply dropped.

What ntfy.sh can see: that some (random-looking) topics get posts, when, how
big, from which IP, and the device ids in the `f`/`to` fields. It cannot read
or forge anything. It keeps messages about 12 hours, rate-limits each IP
(IGRIS reports "rate limited" if it happens), and if it is down the devices
simply can't reach each other until it's back. If you'd rather not depend on
it, run your own ntfy server or `igris-relay` and enter its address under
Advanced on **both** devices.

### Relay session (optional, own relay)
1. Device connects (outbound WebSocket; `wss://` required except to loopback).
2. Relay sends `challenge{nonce}`.
3. Device answers `auth{device, key, ts, sig}`: Ed25519 signature over
   `igris-relay-auth-v1 ‖ nonce ‖ device ‖ ts`. The relay checks the id is the
   key's hash, the signature, and the clock (±5 min).
4. `ready{expires_at}` — sessions last 12 h, then the device re-authenticates.
   No bearer tokens exist.
5. Device sends `peers{ids}` (its trusted devices). The relay routes only
   between two devices that **both** list each other, and only tells such
   peers about presence. It never reveals IP addresses.

### Envelope (every device-to-device message)
`{v, id, from, to, ts, exp, epk, nonce, ct, sig}`
* Payload (including the message type) encrypted to the recipient:
  ephemeral X25519 with the recipient's key → HKDF-SHA256 (salt = message id,
  info binds both device ids and both public keys) → ChaCha20-Poly1305 with the
  header as associated data.
* Ed25519 signature over the header and ciphertext.
* Receiver checks: version, addressed to it, sender = the trusted key it
  holds, owner, freshness (`exp` ≤ 15 min after `ts`, not expired, clock skew
  ≤ 5 min), signature — then records the id (replay table) and decrypts.
* All primitives are from audited RustCrypto/dalek crates.

### Messages
`hello`, `task_request`, `task_ack`, `task_update{seq}`, `task_control`,
`control_result`, `approval_needed`, `approval_answer`, `device_announce`,
`device_revoked`, `sync_batch`, `sync_ack`.

### Remote task lifecycle
* Requester stores the request (`remote_tasks`, outgoing): `pending` → `sent`
  (relay delivered) or `queued` (target offline, relay holds ≤10 min) →
  `accepted` (target's real acknowledgement) → `running` /
  `waiting_for_approval` / `paused` → `completed` / `ended` / `failed` /
  `cancelled`, or `rejected` / `timed_out` (nobody picked it up in 10 min).
* Executor runs it as an **ordinary conversation** through `chat::generate`
  → orchestrator → tool executor, with its own permission policy. Its
  orchestrator events become `task_update`s with an increasing persisted
  `seq`; the requester ignores older/duplicate updates. The final status is the
  orchestrator task's own final state (e.g. `completed` only after the
  executor's verification passed).
* Duplicate `task_request`s (same id) are acknowledged again, never run twice.
* Pause / resume / stop go through `Orchestrator::control` — the same path as
  the local UI; resume re-observes before acting.
* If IGRIS closes on the executor mid-task, it reports `failed` ("closed while
  working on this") on next start.

### Remote approvals
When an action needs approval on the executor, it sends `approval_needed`
(task, call id, tool, description, permission, SHA-256 digest of the canonical
arguments, target device, expiry) to the requester and shows its own local
prompt. The user's answer on the requester is a **signed approval artifact**
binding approval id, request, task, call, tool, argument digest, target
device, approver device, decision, issue time and expiry. The executor
verifies the signature against its registry, that the approver is the device
that requested the task, every binding against the action actually pending,
the expiry, and consumes the approval id (single use). Whichever answers
first — local UI or verified remote approval — decides. Nothing is ever
auto-approved; SENSITIVE/CRITICAL semantics are unchanged.

### Pairing
1. Inviter: `pair_open{pid}` (relay confirms before the code is shown).
2. Joiner (types the code): `pair_send{pid, blob}` where blob = its public
   keys + capabilities, signed, encrypted with HKDF(secret, salt=pid, "join").
3. Inviter decrypts (each failure counts; 5 → code dead), shows the
   confirmation. On "Allow": trusts the joiner, replies `pair_reply` with its
   own keys, owner id and the user's other devices (signed, encrypted with
   "accept"), and announces the new device to the others.
4. Joiner verifies, adopts the owner id, trusts the inviter and the listed devices.

### Memory sync
Memories only. Each has `sync_id`, `revision`, `origin`. Local edits bump the
revision; deletes leave tombstones. Changes are sent in local-clock order in
batches of 50 and acknowledged per peer. Conflict rule: higher revision wins;
on a tie, the greater device id wins — both sides apply the same rule and
converge. Incoming content passes the same validation as local input (length,
credential detection). Equivalent memories added independently on two devices
are kept as two rows. **Never synced:** SQLite files, conversations, settings,
API keys, device keys, screenshots, attachments, tasks.

## Own relay (optional)

Not needed for normal use. `igris-relay` is a separate ~500-line binary. It authenticates devices, routes
envelopes, reports presence, holds ≤100 messages per offline device for ≤10
minutes, meets devices for pairing, rate-limits (200 frames / 10 s), limits
frames to 256 KB. It persists nothing and cannot read anything. It does not
execute tasks or decide permissions.

```text
cargo run --release -p igris-relay -- --listen 127.0.0.1:8787
# optional: only these devices may connect
cargo run --release -p igris-relay -- --allow dev_…,dev_…
```

Run it on a machine both devices can reach (a small VPS, or a home server)
behind a TLS reverse proxy, e.g. Caddy:

```text
relay.example.com {
    reverse_proxy 127.0.0.1:8787
}
```

Then in IGRIS on each device: Settings → Devices → Advanced (phone: More →
Phone and computer → Advanced) → `wss://relay.example.com`.

## Using it

1. Open IGRIS on both devices.
2. On the PC: Settings → Devices → **Show a pairing code**. On the phone:
   More → Phone and computer → type the code → **Join**. On the PC: **Allow**.
   That's all the setup; both devices stay paired until you remove one.
3. On the phone, ask IGRIS "open Notepad on my PC", or use **Ask My PC** on the
   Devices screen. You'll see "Sent to My PC" → "My PC accepted the task" →
   "Done — completed on My PC" (or what really happened).
4. Approve the PC's actions on the phone when asked. Pause/stop from the phone.
5. Remove a device from either side; from the PC it is removed everywhere.

## Notifications

System notifications (desktop and Android) for meaningful events only:
accepted, needs your OK, done, failed, didn't respond, device added/removed.
There is **no push service**: a device whose IGRIS app isn't running is not
notified, and the PC must have IGRIS running to receive tasks.

## Limits (honest)

* By default the devices talk through the public ntfy.sh service (it sees
  only ciphertext and metadata, see Transport); it can rate-limit or be down.
* No push notifications; Android may suspend the app in the background, so
  the phone is reliably reachable only while IGRIS is open.
* Screen content is never sent between devices; there's no remote viewing.
* Conversations are not synced (each device keeps its own; a task from the
  phone appears on the PC as its own conversation).
* Linux/macOS builds keep device keys without OS protection.
* A device can be part of one IGRIS at a time; joining another requires
  removing its devices first.
* **Not yet tested on a physical Android phone talking to a physical Windows
  PC over the internet** (see Test results for what has been run).

## Test results

| Test | Where | Result |
|---|---|---|
| Crypto, pairing, approvals, replay, registry, sync, targeting (unit) | `cargo test -p igris-core device::` | PASS |
| Relay over real sockets: auth, mutual routing, presence, offline queue, pairing limits, floods | `igris-relay/tests/relay.rs` | PASS (6) |
| Two IGRIS hubs through an ntfy-compatible server on loopback (`device/ntfy_mock.rs`): pairing with only the code (cross-device switched on by pairing itself) and a task with a signed remote approval, with a check that every post is ciphertext; a closed PC gets the task when it opens; a 60-memory sync split into parts | `igris-core/src/device/e2e_tests.rs` (`over_ntfy_*`) | PASS (3) |
| The same against the real **ntfy.sh** | `live_ntfy_sh_pairing_and_a_task` (CI job `ntfy-live`; can't run from the development sandbox, whose network blocks ntfy.sh) | see CI |
| Two IGRIS hubs + real relay on loopback: pairing with confirmation; a task from the "phone" run on the "PC" through the real chat → orchestrator → executor path (scripted model) with a signed remote approval, verified by the PC's read-back; remote deny; remote stop; offline PC gets the queued task on reconnect; memory sync both ways incl. delete; revocation; wrong code / denied pairing; no relay | `igris-core/src/device/e2e_tests.rs` | PASS (7) |
| DPAPI seal/unseal round trip | Windows CI runner | PASS |
| **Real Android runtime (ntfy.sh, the default):** the same flow as below, but the phone types only the code and taps **Join** — no address | `scripts/android-cross-device.sh` | see Android workflow |
| **Real Android runtime (relay, before ntfy):** the APK on an Android 14 emulator seals its device keys with the Android Keystore, connects to a real relay on the CI host, pairs with a real IGRIS device hub ("CI PC", `igris-core/examples/device_peer.rs`) by typing the code in the phone UI, sends a task from the phone UI, approves the PC's action on the phone (signed approval verified on the PC) and shows "Done — completed on CI PC" | `scripts/android-cross-device.sh`, Android workflow run 37605750133 | PASS |

The CI PC's task runner stands in for the AI model (it asks for one approval and
reports the result); the real chat path is covered by the loopback tests above.

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| "Rate limited" | ntfy.sh limits how much one internet connection may post; wait a minute. Many devices behind one IP, or very large memory syncs, make it likelier. |
| Can't connect (ntfy.sh) | Something blocks `ntfy.sh` (some office/school networks); try another network, or use your own server under Advanced. |
| "Use a secure relay address (wss://)" | Only `wss://` (or `ws://` to this same machine) is accepted. |
| "That address isn't an IGRIS relay" | Wrong URL, or the proxy doesn't forward WebSocket upgrades. |
| "The relay refused this device" | The relay runs with `--allow` and this device isn't listed (its id is in Settings → Devices → This device). |
| "No device is waiting for that code" / pairing times out | The code expired (10 min), was used up (5 tries), IGRIS on the PC was closed, or the two devices use different server addresses (both must be empty, or the same). Show a new code. |
| Task stays "queued" | The target device is offline; it gets the task if it connects within 10 minutes. |
| "Clock too far off" | A device's clock is >5 minutes wrong; fix the system time. |
