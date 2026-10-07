import {
  Laptop,
  Link2,
  Pause,
  Play,
  ShieldCheck,
  Smartphone,
  Square,
} from "lucide-react";
import { useEffect, useState } from "react";
import { useDeviceStore } from "@/stores/deviceStore";
import {
  CAPABILITY_LABEL,
  isActive,
  keyProtectionText,
  linkText,
  platformLabel,
  taskStatusText,
  taskTone,
} from "@/components/devices/deviceText";
import { Dot, Screen, Section, Switch } from "../ui";

/** More → Your devices: connect, pair with your PC, send tasks, see what's happening. */
export function DevicesScreen() {
  const {
    overview,
    error,
    busy,
    refresh,
    configure,
    connectAndJoin,
    startPairing,
    cancelPairing,
    pairing,
    pairingResult,
    revoke,
    sendTask,
    control,
  } = useDeviceStore();
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
      <Screen title="Your devices" back="/more">
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

  return (
    <Screen
      title="Your devices"
      subtitle="Use IGRIS on your phone and your computer together."
      back="/more"
    >
      {error && (
        <p role="alert" className="text-sm" style={{ color: "var(--m-bad)" }}>
          {error}
        </p>
      )}

      {devices.length === 0 && (
        <Section title="Connect to your computer">
          <div className="m-card space-y-3 p-4">
            <p className="m-muted text-sm">
              On your computer, open IGRIS → Settings → Devices → "Show a
              pairing code". Type that code here.
            </p>
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
              onClick={() =>
                void connectAndJoin(
                  showAdvanced ? relay.trim() : (s.relayUrl ?? ""),
                  code.trim(),
                ).then((ok) => ok && setCode(""))
              }
            >
              <Link2 className="size-4" />{" "}
              {busy ? "Waiting for your computer to allow it…" : "Join"}
            </button>
            {pairingResult && (
              <p
                className="text-sm"
                style={{
                  color: pairingResult.ok ? "var(--m-ok)" : "var(--m-bad)",
                }}
              >
                {pairingResult.message}
              </p>
            )}
            <button
              type="button"
              className="m-muted text-sm underline"
              onClick={() => setAdvanced(!showAdvanced)}
            >
              {showAdvanced ? "Hide advanced" : "Advanced: use my own server"}
            </button>
            {showAdvanced && (
              <>
                <p className="m-muted text-sm">
                  Leave empty to use ntfy.sh (the default) — your computer's
                  Devices page must also have it empty. Or enter your own
                  ntfy server (https://…) or IGRIS relay (wss://…) — the same
                  one your computer uses.
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
          </div>
        </Section>
      )}

      {devices.length > 0 && target && (
        <Section title={`Ask ${target.name}`}>
          <div className="m-card space-y-3 p-4">
            <textarea
              aria-label="Task"
              className="m-input min-h-20"
              placeholder={`e.g. "Open Notepad" or "Find yesterday's report"`}
              value={text}
              maxLength={2000}
              onChange={(e) => setText(e.target.value)}
            />
            <button
              type="button"
              className="m-button m-button-primary w-full"
              disabled={busy || !text.trim()}
              onClick={() =>
                void sendTask(target.deviceId, text.trim()).then(
                  (t) => t && setText(""),
                )
              }
            >
              Send to {target.name}
            </button>
            {!connected && (
              <p className="m-muted text-sm">
                Not connected right now — it will be sent when the connection is
                back (within 10 minutes).
              </p>
            )}
          </div>
        </Section>
      )}

      {overview.tasks.length > 0 && (
        <Section title="Recent">
          <div className="m-card">
            {overview.tasks.slice(0, 8).map((t) => (
              <div key={t.requestId} className="m-row items-start">
                <span className="mt-2">
                  <Dot tone={taskTone(t)} />
                </span>
                <div className="min-w-0 flex-1">
                  <div className="font-medium break-words">{t.objective}</div>
                  <div className="m-muted text-sm">{taskStatusText(t)}</div>
                  {t.detail && !t.detail.startsWith(taskStatusText(t)) && (
                    <div className="m-faint mt-0.5 text-sm break-words">
                      {t.detail}
                    </div>
                  )}
                  {t.direction === "outgoing" && isActive(t) && (
                    <div className="mt-2 flex gap-2">
                      {t.status === "paused" ? (
                        <button
                          type="button"
                          className="m-button"
                          onClick={() => void control(t.requestId, "resume")}
                        >
                          <Play className="size-4" /> Resume
                        </button>
                      ) : (
                        <button
                          type="button"
                          className="m-button"
                          onClick={() => void control(t.requestId, "pause")}
                        >
                          <Pause className="size-4" /> Pause
                        </button>
                      )}
                      <button
                        type="button"
                        className="m-button"
                        onClick={() => void control(t.requestId, "stop")}
                      >
                        <Square className="size-4" /> Stop
                      </button>
                    </div>
                  )}
                </div>
              </div>
            ))}
          </div>
        </Section>
      )}

      <Section title="Paired">
        <div className="m-card">
          {devices.length === 0 && (
            <p className="m-row m-muted text-sm">No devices yet.</p>
          )}
          {devices.map((d) => (
            <div key={d.deviceId} className="m-row items-start">
              <span className="m-icon-badge">
                {d.platform === "android" ? (
                  <Smartphone className="size-5" />
                ) : (
                  <Laptop className="size-5" />
                )}
              </span>
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2 font-medium">
                  {d.name} <Dot tone={d.online ? "ok" : "warn"} />
                </div>
                <div className="m-muted text-sm">
                  {platformLabel(d.platform)} ·{" "}
                  {d.online ? "online" : `${d.name} is offline`}
                </div>
                {d.capabilities.length > 0 && (
                  <div className="m-faint mt-1 text-sm">
                    {d.capabilities
                      .map((c) => CAPABILITY_LABEL[c] ?? c)
                      .join(" · ")}
                  </div>
                )}
                {confirm === d.deviceId ? (
                  <div className="mt-2 flex gap-2">
                    <button
                      type="button"
                      className="m-button flex-1"
                      onClick={() => setConfirm(null)}
                    >
                      Keep
                    </button>
                    <button
                      type="button"
                      className="m-button flex-1"
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
                  <button
                    type="button"
                    className="m-muted mt-1 text-sm underline"
                    onClick={() => setConfirm(d.deviceId)}
                  >
                    Remove from this phone
                  </button>
                )}
              </div>
            </div>
          ))}
        </div>
      </Section>

      {devices.length > 0 && (
        <Section title="Connection">
          <div className="m-card space-y-3 p-4">
            <div className="flex items-center gap-2">
              <Dot tone={link.tone} />
              <span className="text-[15px]">{link.text}</span>
            </div>
            <input
              aria-label="Server address"
              inputMode="url"
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              className="m-input"
              placeholder="Default: ntfy.sh"
              value={relay}
              onChange={(e) => setUrl(e.target.value)}
            />
            <div className="flex gap-2">
              <button
                type="button"
                className="m-button m-button-primary flex-1"
                disabled={busy}
                onClick={() =>
                  void configure(relay.trim() || null, true, s.syncMemory)
                }
              >
                {s.enabled ? "Reconnect" : "Connect"}
              </button>
              {s.enabled && (
                <button
                  type="button"
                  className="m-button flex-1"
                  disabled={busy}
                  onClick={() =>
                    void configure(s.relayUrl, false, s.syncMemory)
                  }
                >
                  Turn off
                </button>
              )}
            </div>
            <p className="m-muted text-sm">
              Leave the address empty to use ntfy.sh (free, nothing to set up).
              Messages are end-to-end encrypted; the service can't read them.
            </p>
          </div>
        </Section>
      )}

      <Section title="Memory sync">
        <div className="m-card">
          <Switch
            label="Sync memories"
            description="Keep what IGRIS remembers the same on your devices. Chats, files, settings and keys are never synced."
            checked={s.syncMemory}
            disabled={busy}
            onChange={(on) => void configure(s.relayUrl, s.enabled, on)}
          />
        </div>
      </Section>

      {devices.length > 0 && (
        <Section title="Add a device">
          <div className="m-card space-y-3 p-4">
            {pairing ? (
              <>
                <p className="m-muted text-sm">On the new device, enter:</p>
                <div className="font-mono text-xl tracking-wider">
                  {pairing.code}
                </div>
                <button
                  type="button"
                  className="m-button w-full"
                  onClick={() => void cancelPairing()}
                >
                  Cancel
                </button>
              </>
            ) : (
              <button
                type="button"
                className="m-button w-full"
                disabled={busy}
                onClick={() => void startPairing()}
              >
                Show a pairing code
              </button>
            )}
            {pairingResult && (
              <p
                className="text-sm"
                style={{
                  color: pairingResult.ok ? "var(--m-ok)" : "var(--m-bad)",
                }}
              >
                {pairingResult.message}
              </p>
            )}
          </div>
        </Section>
      )}

      <Section title="This phone">
        <div className="m-card">
          <div className="m-row items-start">
            <span className="m-icon-badge">
              <ShieldCheck className="size-5" />
            </span>
            <div className="m-muted min-w-0 flex-1 text-sm">
              "{overview.thisDevice.name}" ·{" "}
              {keyProtectionText(overview.thisDevice.keyProtection)}. Its keys
              never leave the phone. Your computer decides what it runs; this
              phone can only ask, approve, pause or stop its own requests.
            </div>
          </div>
        </div>
      </Section>
    </Screen>
  );
}
