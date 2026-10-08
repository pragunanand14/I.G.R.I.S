import { Laptop, Pause, Play, Smartphone, Square } from "lucide-react";
import { useEffect, useState } from "react";
import { useDeviceStore } from "@/stores/deviceStore";
import { CAPABILITY_LABEL, isActive, keyProtectionText, linkText, platformLabel, taskStatusText, taskTone } from "@/components/devices/deviceText";
import { Dot, ErrorText, Group, Screen, Section, Switch } from "../ui";

/** More → Phone and computer: pair with your PC, send it tasks, see what's happening. */
export function DevicesScreen() {
  const { overview, error, busy, refresh, configure, connectAndJoin, startPairing, cancelPairing, pairing, pairingResult, revoke, sendTask, control } =
    useDeviceStore();
  const [url, setUrl] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [text, setText] = useState("");
  const [confirm, setConfirm] = useState<string | null>(null);
  const [advanced, setAdvanced] = useState<boolean | null>(null);
  useEffect(() => {
    void refresh();
  }, [refresh]);

  if (!overview) {
    return (
      <Screen title="Phone and computer" back="/more">
        <p className="m-muted">{error ?? "Starting…"}</p>
      </Screen>
    );
  }
  const s = overview.settings;
  const link = linkText(overview.status);
  const connected = overview.status.state === "connected";
  const devices = overview.devices.filter((d) => d.trusted && !d.revokedAt);
  const relay = url ?? s.relayUrl ?? "";
  const target = devices.find((d) => d.platform !== "android") ?? devices[0];
  // A saved own-server address is always shown, so it can't silently differ from the PC's.
  const showAdvanced = advanced ?? Boolean(s.relayUrl);
  const result = pairingResult && (
    <p className="px-1 text-sm" style={{ color: pairingResult.ok ? "var(--m-ok)" : "var(--m-bad)" }}>
      {pairingResult.message}
    </p>
  );

  return (
    <Screen title="Phone and computer" subtitle={devices.length === 0 ? "Use IGRIS on your phone and your computer together." : undefined} back="/more">
      {error && <ErrorText>{error}</ErrorText>}

      {devices.length === 0 && (
        <Section>
          <p className="m-muted px-1 text-[15px]">On your computer, open IGRIS → Settings → Devices → Show a pairing code. Type that code here.</p>
          <input
            aria-label="Pairing code"
            className="m-input font-mono tracking-wider uppercase"
            placeholder="XXXX-XXXX-XXXX-XXXX-XXXX-XXXX"
            value={code}
            autoCapitalize="characters"
            autoCorrect="off"
            spellCheck={false}
            onChange={(e) => setCode(e.target.value)}
          />
          <button
            type="button"
            className="m-button m-button-primary w-full"
            disabled={busy || code.replace(/[^0-9a-z]/gi, "").length < 24}
            onClick={() => void connectAndJoin(showAdvanced ? relay.trim() : (s.relayUrl ?? ""), code.trim()).then((ok) => ok && setCode(""))}
          >
            {busy ? "Waiting for your computer to allow it…" : "Join"}
          </button>
          {result}
          <button type="button" className="m-faint min-h-10 px-1 text-sm" onClick={() => setAdvanced(!showAdvanced)}>
            {showAdvanced ? "Hide advanced" : "Advanced: use my own server"}
          </button>
          {showAdvanced && (
            <>
              <p className="m-muted px-1 text-sm">
                Leave empty to use ntfy.sh (the default) — your computer's Devices page must also have it empty. Or enter your own ntfy server (https://…)
                or IGRIS relay (wss://…) — the same one your computer uses.
              </p>
              <input
                aria-label="Server address"
                inputMode="url"
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
                className="m-input"
                placeholder="https://ntfy.sh"
                value={relay}
                onChange={(e) => setUrl(e.target.value)}
              />
            </>
          )}
        </Section>
      )}

      {devices.length > 0 && target && (
        <Section title={`Ask ${target.name}`}>
          <textarea
            aria-label="Task"
            className="m-input min-h-28 resize-none py-3"
            placeholder={`e.g. "Open Notepad" or "Find yesterday's report"`}
            value={text}
            maxLength={2000}
            onChange={(e) => setText(e.target.value)}
          />
          <button
            type="button"
            className="m-button m-button-primary w-full"
            disabled={busy || !text.trim()}
            onClick={() => void sendTask(target.deviceId, text.trim()).then((t) => t && setText(""))}
          >
            Send to {target.name}
          </button>
          {!connected && <p className="m-muted px-1 text-sm">Not connected right now — it will be sent when the connection is back (within 10 minutes).</p>}
        </Section>
      )}

      {overview.tasks.length > 0 && (
        <Section title="Recent">
          <Group>
            {overview.tasks.slice(0, 8).map((t) => (
              <div key={t.requestId} className="m-row items-start">
                <span className="mt-2">
                  <Dot tone={taskTone(t)} />
                </span>
                <div className="min-w-0 flex-1">
                  <div className="font-medium break-words">{t.objective}</div>
                  <div className="m-muted text-sm">{taskStatusText(t)}</div>
                  {t.detail && !t.detail.startsWith(taskStatusText(t)) && <div className="m-faint mt-0.5 text-sm break-words">{t.detail}</div>}
                  {t.direction === "outgoing" && isActive(t) && (
                    <div className="mt-2 flex gap-2">
                      {t.status === "paused" ? (
                        <button type="button" className="m-button min-h-9 px-3 text-sm" onClick={() => void control(t.requestId, "resume")}>
                          <Play className="size-4" /> Resume
                        </button>
                      ) : (
                        <button type="button" className="m-button min-h-9 px-3 text-sm" onClick={() => void control(t.requestId, "pause")}>
                          <Pause className="size-4" /> Pause
                        </button>
                      )}
                      <button type="button" className="m-button min-h-9 px-3 text-sm" onClick={() => void control(t.requestId, "stop")}>
                        <Square className="size-4" /> Stop
                      </button>
                    </div>
                  )}
                </div>
              </div>
            ))}
          </Group>
        </Section>
      )}

      {devices.length > 0 && (
        <Section title="Paired">
          <Group>
            {devices.map((d) => (
              <div key={d.deviceId} className="m-row items-start">
                <span className="m-muted mt-0.5 shrink-0">{d.platform === "android" ? <Smartphone className="size-5" /> : <Laptop className="size-5" />}</span>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2 font-medium">
                    {d.name} <Dot tone={d.online ? "ok" : "warn"} />
                  </div>
                  <div className="m-muted text-sm">
                    {platformLabel(d.platform)} · {d.online ? "online" : `${d.name} is offline`}
                  </div>
                  {d.capabilities.length > 0 && <div className="m-faint mt-1 text-sm">{d.capabilities.map((c) => CAPABILITY_LABEL[c] ?? c).join(" · ")}</div>}
                  {confirm === d.deviceId ? (
                    <div className="mt-2 flex gap-2">
                      <button type="button" className="m-button min-h-9 flex-1 text-sm" onClick={() => setConfirm(null)}>
                        Keep
                      </button>
                      <button
                        type="button"
                        className="m-button min-h-9 flex-1 text-sm"
                        style={{ color: "var(--m-bad)" }}
                        onClick={() => {
                          setConfirm(null);
                          void revoke(d.deviceId);
                        }}
                      >
                        Remove
                      </button>
                    </div>
                  ) : (
                    <button type="button" className="m-faint mt-1 min-h-9 text-sm" onClick={() => setConfirm(d.deviceId)}>
                      Remove from this phone
                    </button>
                  )}
                </div>
              </div>
            ))}
          </Group>
        </Section>
      )}

      {devices.length > 0 && (
        <Section title="Connection">
          <Group>
            <div className="m-row">
              <Dot tone={link.tone} />
              <span className="min-w-0 flex-1 break-words">{link.text}</span>
            </div>
            <Switch label="Sync memories" description="Keep what IGRIS remembers the same on your devices. Chats, files, settings and keys are never synced." checked={s.syncMemory} disabled={busy} onChange={(on) => void configure(s.relayUrl, s.enabled, on)} />
            <div className="m-row py-0">
              <input
                aria-label="Server address"
                inputMode="url"
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
                className="m-field"
                placeholder="Server: ntfy.sh (default)"
                value={relay}
                onChange={(e) => setUrl(e.target.value)}
              />
            </div>
          </Group>
          <div className="flex gap-2">
            <button type="button" className="m-button flex-1" disabled={busy} onClick={() => void configure(relay.trim() || null, true, s.syncMemory)}>
              {s.enabled ? "Reconnect" : "Connect"}
            </button>
            {s.enabled && (
              <button type="button" className="m-button flex-1" disabled={busy} onClick={() => void configure(s.relayUrl, false, s.syncMemory)}>
                Turn off
              </button>
            )}
          </div>
          <p className="m-faint px-1 text-sm">Messages are end-to-end encrypted; the service that carries them can't read them.</p>
        </Section>
      )}

      {devices.length > 0 && (
        <Section title="Add a device">
          {pairing ? (
            <div className="m-card space-y-3 p-4">
              <p className="m-muted text-sm">On the new device, enter:</p>
              <div className="font-mono text-xl tracking-wider" data-selectable>
                {pairing.code}
              </div>
              <button type="button" className="m-button w-full" onClick={() => void cancelPairing()}>
                Cancel
              </button>
            </div>
          ) : (
            <button type="button" className="m-button w-full" disabled={busy} onClick={() => void startPairing()}>
              Show a pairing code
            </button>
          )}
          {result}
        </Section>
      )}

      <p className="m-faint px-1 text-sm">
        "{overview.thisDevice.name}" · {keyProtectionText(overview.thisDevice.keyProtection)}. Its keys never leave the phone. Your computer decides what it
        runs; this phone can only ask, approve, pause or stop its own requests.
      </p>
    </Screen>
  );
}
