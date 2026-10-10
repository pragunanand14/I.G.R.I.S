# Smart home

IGRIS controls devices on your home network directly — no vendor app, no cloud
in between. Today: **LG TVs (webOS)**. Other devices are added as drivers to
the same layer (store, AI tools and UI are shared).

## LG TV

**Set up** (desktop: Smart home in the top bar; phone: the TV icon on Home):

1. Turn the TV on; it must be on the same Wi-Fi as the PC or phone.
2. **Find my TV** (or type its IP address: TV → Settings → Network → Wi-Fi
   connection → Advanced settings) → **Pair**.
3. The TV asks whether IGRIS may control it — press **Allow** on the TV.

To switch the TV **on** remotely, enable *Turn on via Wi-Fi* (or *Mobile TV
On*) in the TV's settings; IGRIS learns the TV's network address while pairing
and sends a Wake-on-LAN signal.

**What it can do:** power off/on, volume up/down/set/mute, channel up/down,
play/pause/stop/rewind/fast-forward, open an installed app (Netflix, YouTube…),
switch input (HDMI…), remote buttons (arrows, OK, back, home, menu, numbers),
and show a message on the TV. From the remote screen, or by asking IGRIS
("turn the TV volume down", "open YouTube on the TV").

### How it works

The TV runs LG's "SSAP" WebSocket service on port 3000 (`ws://`) and 3001
(`wss://`, self-signed certificate, required by newer firmware). The first
connection registers IGRIS with a fixed permission list; the TV prompts the
user and returns a client key that later connections present. The
certificate seen at pairing is **pinned** (SHA-256 stored); later connections
with any other certificate are refused. Remote buttons go over a second
socket the TV hands out. Discovery is an SSDP search for LG's
`webos-second-screen` service. Protocol details follow the open-source
aiowebostv client (Apache-2.0).

### Safety

* Only devices the user paired (with Allow on the TV) can be controlled.
* The AI gets two tools: `home_devices` (SAFE, read-only) and `tv_control`
  (LOW: a fixed list of actions, validated, through the tool executor and
  audit log). No arbitrary commands reach the TV.
* The client key, MAC address and pinned certificate stay in the local
  database on the native side; they are never sent to the UI or the model.
* A TV that's off or unreachable reads as "off or not reachable"; every
  action reports what the TV actually answered.

### Tested

| Test | Result |
|---|---|
| Pairing (Allow / Decline), key kept private, no re-prompt, every action, button socket, errors in plain words, state, unreachable TV — against a simulated webOS TV (`igris-core/src/home/tests.rs`) | PASS |
| SSDP discovery against a simulated TV; certificate pinning (accept the paired certificate, refuse another) | PASS |
| Remote screen: add/pair flow, actions and errors, off TV (`src/components/home/SmartHome.test.tsx`) | PASS |
| **A real LG TV** | **not yet** — please report how pairing and each control behave on your model. |

### Limits

* LG webOS TVs only (for now). Other brands each need their own driver.
* Discovery may not find the TV on networks that block multicast (some
  routers, guest Wi-Fi); enter the IP address instead.
* Switching on needs Wake-on-LAN support on the TV and the same network.
