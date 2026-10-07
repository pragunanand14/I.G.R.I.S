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
| Relay link | `device/link.rs` | Outbound authenticated WebSocket to the relay, reconnect with backoff. |
| Hub | `device/hub.rs` | Routing: remote tasks through the existing orchestrator, control, approvals, presence, pairing, memory sync. |
| Memory sync | `device/sync.rs` | Revisions + tombstones; memories only. |
| Targeting | `device/target.rs` | "on my laptop", "here", "my phone", names; asks when ambiguous. |
| Tools | `igris-core/src/tools/devices.rs` | `list_devices` (SAFE), `send_to_device` (LOW), `device_task_status` (SAFE), `device_task_control` (LOW). |
| Relay | `igris-relay/` | Separate minimal server binary. |
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
* **Private keys never leave the device** and are never sent to the relay.
  At rest: DPAPI (Windows, sealed to the user account), Android Keystore
  (non-exportable AES-GCM key), or — on Linux/macOS builds — stored unsealed
  in the app database (reported as such in the UI).

## Protocol

### Relay session
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

## Relay

`igris-relay` is a separate ~500-line binary. It authenticates devices, routes
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

Then in IGRIS on each device: Settings → Devices (phone: More → Your
devices) → relay address `wss://relay.example.com` → Connect.

## Using it

1. Connect both devices to the relay.
2. On the PC: **Show a pairing code**. On the phone: type it under **Join your
   computer**. On the PC: **Allow**.
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

* No push notifications; Android may suspend the app in the background, so
  the phone is reliably reachable only while IGRIS is open.
* Screen content is never sent between devices; there's no remote viewing.
* Conversations are not synced (each device keeps its own; a task from the
  phone appears on the PC as its own conversation).
* Linux/macOS builds keep device keys without OS protection.
* A device can be part of one IGRIS at a time; joining another requires
  removing its devices first.
* Tested: unit tests, a real relay with two IGRIS hubs on loopback (pairing,
  remote task through the real chat/orchestrator path, signed approval, deny,
  stop, offline queue, memory sync, revocation), relay security tests, DPAPI on
  the Windows CI runner. **Not yet tested on a physical Android phone talking to
  a physical Windows PC over the internet.**

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| "Use a secure relay address (wss://)" | Only `wss://` (or `ws://` to this same machine) is accepted. |
| "That address isn't an IGRIS relay" | Wrong URL, or the proxy doesn't forward WebSocket upgrades. |
| "The relay refused this device" | The relay runs with `--allow` and this device isn't listed (its id is in Settings → Devices → This device). |
| "No device is waiting for that code" | The code expired (10 min), was used up (5 tries), or the PC isn't connected. Show a new code. |
| Task stays "queued" | The target device is offline; it gets the task if it connects within 10 minutes. |
| "Clock too far off" | A device's clock is >5 minutes wrong; fix the system time. |
