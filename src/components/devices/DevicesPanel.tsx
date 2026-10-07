import { Link2, Monitor, Pause, Play, ShieldCheck, Smartphone, Square, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { Panel } from "@/components/ui/Panel";
import { StatusDot } from "@/components/ui/StatusDot";
import { Toggle } from "@/components/ui/Toggle";
import { INPUT } from "@/components/productivity/styles";
import { useDeviceStore } from "@/stores/deviceStore";
import type { DevicesOverview, PairedDevice } from "@/types/devices";
import { CAPABILITY_LABEL, isActive, keyProtectionText, linkText, platformLabel, taskStatusText, taskTone } from "./deviceText";

const BUTTON = "rounded-md border border-line px-2.5 py-1 text-xs text-muted hover:border-accent hover:text-accent disabled:opacity-40";
const PRIMARY = "rounded-md bg-accent px-3 py-1 text-xs font-semibold text-bg disabled:opacity-40";
const toneDot = (t: "ok" | "warn" | "bad") => (t === "bad" ? "error" : t);

/** Settings → Devices: this device, the relay connection, paired devices, pairing, tasks between devices. */
export function DevicesPanel({ disabled }: { disabled: boolean }) {
  const overview = useDeviceStore((s) => s.overview);
  const error = useDeviceStore((s) => s.error);
  const refresh = useDeviceStore((s) => s.refresh);
  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <Panel title="Devices" action={overview && <ConnectionBadge overview={overview} />}>
      {error && (
        <p role="alert" className="mb-3 rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">
          {error}
        </p>
      )}
      {!overview ? (
        <p className="text-sm text-muted">Cross-device features are starting…</p>
      ) : (
        <div className="space-y-5">
          <p className="text-xs text-muted">
            Use IGRIS across your own devices: send a task from your phone to this PC, approve actions from either one, keep memories in sync.
            Pair with a code — nothing else to set up. Messages are end-to-end encrypted on your devices; the service that carries them (ntfy.sh by
            default) can't read them or run anything.
          </p>
          <ThisDeviceSection overview={overview} disabled={disabled} />
          <ConnectionSection overview={overview} disabled={disabled} />
          <PairedSection overview={overview} disabled={disabled} />
          <PairingSection overview={overview} disabled={disabled} />
          <TasksSection overview={overview} disabled={disabled} />
        </div>
      )}
    </Panel>
  );
}

function ConnectionBadge({ overview }: { overview: DevicesOverview }) {
  const l = linkText(overview.status);
  return (
    <span className="flex items-center gap-1.5 text-xs text-muted">
      <StatusDot tone={toneDot(l.tone)} /> {overview.status.state === "connected" ? "Connected" : overview.status.state === "off" ? "Off" : overview.status.state === "connecting" ? "Connecting" : "Offline"}
    </span>
  );
}

function Heading({ children }: { children: string }) {
  return <h3 className="mb-2 text-[11px] font-semibold tracking-wider text-faint uppercase">{children}</h3>;
}

function ThisDeviceSection({ overview, disabled }: { overview: DevicesOverview; disabled: boolean }) {
  const renameThis = useDeviceStore((s) => s.renameThis);
  const me = overview.thisDevice;
  const [name, setName] = useState(me.name);
  return (
    <section>
      <Heading>This device</Heading>
      <div className="flex flex-wrap items-center gap-2">
        <input
          aria-label="This device's name"
          className={`${INPUT} w-56`}
          value={name}
          maxLength={40}
          disabled={disabled}
          onChange={(e) => setName(e.target.value)}
          onBlur={() => name.trim() && name.trim() !== me.name && void renameThis(name.trim())}
        />
        <span className="text-xs text-muted">{platformLabel(me.platform)}</span>
      </div>
      <p className="mt-1.5 flex items-center gap-1.5 text-xs text-muted">
        <ShieldCheck className="size-3.5" /> Device keys: {keyProtectionText(me.keyProtection)}. They never leave this device.
      </p>
    </section>
  );
}

function ConnectionSection({ overview, disabled }: { overview: DevicesOverview; disabled: boolean }) {
  const configure = useDeviceStore((s) => s.configure);
  const busy = useDeviceStore((s) => s.busy);
  const s = overview.settings;
  const [url, setUrl] = useState(s.relayUrl ?? "");
  const [advanced, setAdvanced] = useState(Boolean(s.relayUrl));
  const l = linkText(overview.status);
  return (
    <section>
      <Heading>Connection</Heading>
      <p className="flex items-center gap-1.5 text-xs text-muted">
        <StatusDot tone={toneDot(l.tone)} />{" "}
        {overview.status.state === "off"
          ? "Off — it turns on by itself when you pair a device."
          : `${l.text}${s.relayUrl ? "" : " (through ntfy.sh, free — nothing to set up)"}`}
      </p>
      {s.enabled && (
        <button type="button" className={`${BUTTON} mt-2`} disabled={disabled || busy} onClick={() => void configure(s.relayUrl, false, s.syncMemory)}>
          Turn off
        </button>
      )}
      <div className="mt-2 flex items-center justify-between gap-4 text-sm">
        <div>
          <div className="text-fg">Sync memories</div>
          <div className="text-xs text-muted">What IGRIS remembers about you, kept the same on your devices. Chats, files, settings and keys are never synced.</div>
        </div>
        <Toggle label="Sync memories" checked={s.syncMemory} disabled={disabled || busy} onChange={(on) => void configure(s.relayUrl, s.enabled, on)} />
      </div>
      <button type="button" className="mt-2 text-xs text-muted underline" onClick={() => setAdvanced(!advanced)}>
        {advanced ? "Hide advanced" : "Advanced: use my own server"}
      </button>
      {advanced && (
        <div className="mt-2">
          <p className="mb-1.5 text-xs text-muted">
            Empty = ntfy.sh (default). Or your own ntfy server (https://…) or IGRIS relay (wss://…). Use the same on all your devices.
          </p>
          <div className="flex flex-wrap items-center gap-2">
            <input
              aria-label="Server address"
              className={`${INPUT} min-w-0 flex-1`}
              placeholder="https://ntfy.sh"
              value={url}
              disabled={disabled}
              onChange={(e) => setUrl(e.target.value)}
            />
            <button type="button" className={PRIMARY} disabled={disabled || busy} onClick={() => void configure(url.trim() || null, true, s.syncMemory)}>
              Save and connect
            </button>
          </div>
        </div>
      )}
    </section>
  );
}

function DeviceIcon({ d }: { d: { platform: string } }) {
  return d.platform === "android" ? <Smartphone className="size-4" /> : <Monitor className="size-4" />;
}

function PairedSection({ overview, disabled }: { overview: DevicesOverview; disabled: boolean }) {
  const revoke = useDeviceStore((s) => s.revoke);
  const forget = useDeviceStore((s) => s.forget);
  const [confirm, setConfirm] = useState<string | null>(null);
  const devices = overview.devices;
  return (
    <section>
      <Heading>Your devices</Heading>
      {devices.length === 0 ? (
        <p className="text-sm text-muted">No other devices yet. Pair one below.</p>
      ) : (
        <ul className="divide-y divide-line rounded-lg border border-line">
          {devices.map((d: PairedDevice) => {
            const active = d.trusted && !d.revokedAt;
            return (
              <li key={d.deviceId} className="flex flex-wrap items-start gap-3 px-3 py-2.5">
                <span className="mt-0.5 text-muted">
                  <DeviceIcon d={d} />
                </span>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2 text-sm text-fg">
                    {d.name}
                    {active ? (
                      <span className="flex items-center gap-1 text-xs text-muted">
                        <StatusDot tone={d.online ? "ok" : "idle"} /> {d.online ? "Online" : "Offline"}
                      </span>
                    ) : (
                      <span className="text-xs text-danger">Removed</span>
                    )}
                  </div>
                  <div className="text-xs text-muted">
                    {platformLabel(d.platform)}
                    {d.lastSeen ? ` · last seen ${new Date(d.lastSeen).toLocaleString()}` : ""}
                  </div>
                  {active && d.capabilities.length > 0 && (
                    <div className="mt-1 flex flex-wrap gap-1">
                      {d.capabilities.map((c) => (
                        <span key={c} className="rounded border border-line px-1.5 py-0.5 text-[10px] text-muted">
                          {CAPABILITY_LABEL[c] ?? c}
                        </span>
                      ))}
                    </div>
                  )}
                </div>
                {active ? (
                  confirm === d.deviceId ? (
                    <div className="flex gap-1.5">
                      <button type="button" className={BUTTON} onClick={() => setConfirm(null)}>
                        Keep
                      </button>
                      <button
                        type="button"
                        className="rounded-md bg-danger px-2.5 py-1 text-xs font-semibold text-bg"
                        disabled={disabled}
                        onClick={() => {
                          setConfirm(null);
                          void revoke(d.deviceId);
                        }}
                      >
                        Remove
                      </button>
                    </div>
                  ) : (
                    <button type="button" className={BUTTON} disabled={disabled} onClick={() => setConfirm(d.deviceId)} aria-label={`Remove ${d.name}`}>
                      <Trash2 className="size-3.5" />
                    </button>
                  )
                ) : (
                  <button type="button" className={BUTTON} disabled={disabled} onClick={() => void forget(d.deviceId)}>
                    Forget
                  </button>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {confirm && <p className="mt-1.5 text-xs text-muted">Removing a device stops anything it asked this device to do, and it can't connect again unless you pair it again.</p>}
    </section>
  );
}

function PairingSection({ overview, disabled }: { overview: DevicesOverview; disabled: boolean }) {
  const { pairing, startPairing, cancelPairing, join, busy, pairingResult, pairingRequests, confirmPairing } = useDeviceStore();
  const [code, setCode] = useState("");
  const hasDevices = overview.devices.some((d) => d.trusted && !d.revokedAt);
  return (
    <section>
      <Heading>Pair a device</Heading>
      {pairing ? (
        <div className="rounded-lg border border-accent/40 bg-accent/5 p-3">
          <div className="text-xs text-muted">On your phone, open IGRIS → More → Phone and computer, type this code and tap Join:</div>
          <div className="my-2 font-mono text-xl tracking-wider text-fg" data-selectable>
            {pairing.code}
          </div>
          <div className="flex items-center justify-between text-xs text-muted">
            <span>Valid for 10 minutes, once. You'll be asked to allow the device before it's trusted.</span>
            <button type="button" className={BUTTON} onClick={() => void cancelPairing()}>
              Cancel
            </button>
          </div>
          {pairingRequests.map((p) => (
            <div key={p.pairingId} className="mt-3 flex flex-wrap items-center gap-2 border-t border-line pt-3 text-sm text-fg">
              <span className="flex-1">
                Allow "{p.deviceName}" ({platformLabel(p.platform)}) to join your IGRIS?
              </span>
              <button type="button" className={BUTTON} onClick={() => void confirmPairing(p.pairingId, false)}>
                Deny
              </button>
              <button type="button" className={PRIMARY} onClick={() => void confirmPairing(p.pairingId, true)}>
                Allow
              </button>
            </div>
          ))}
        </div>
      ) : (
        <div className="flex flex-wrap items-center gap-2">
          <button type="button" className={PRIMARY} disabled={disabled || busy} onClick={() => void startPairing()}>
            <Link2 className="mr-1 inline size-3.5" /> Show a pairing code
          </button>
          {!hasDevices && (
            <>
              <span className="text-xs text-faint">or</span>
              <input aria-label="Pairing code" className={`${INPUT} w-60 font-mono`} placeholder="Code from your other device" value={code} disabled={disabled} onChange={(e) => setCode(e.target.value)} />
              <button
                type="button"
                className={BUTTON}
                disabled={disabled || busy || code.trim().length < 24}
                onClick={() => void join(code.trim()).then((ok) => ok && setCode(""))}
              >
                {busy ? "Waiting for the other device…" : "Join"}
              </button>
            </>
          )}
        </div>
      )}
      {pairingResult && <p className={`mt-2 text-xs ${pairingResult.ok ? "text-success" : "text-danger"}`}>{pairingResult.message}</p>}
    </section>
  );
}

function TasksSection({ overview, disabled }: { overview: DevicesOverview; disabled: boolean }) {
  const { sendTask, control, busy } = useDeviceStore();
  const targets = overview.devices.filter((d) => d.trusted && !d.revokedAt);
  const [target, setTarget] = useState("");
  const [text, setText] = useState("");
  const chosen = target || targets[0]?.deviceId || "";
  return (
    <section>
      <Heading>Tasks between devices</Heading>
      {targets.length > 0 && (
        <div className="mb-3 flex flex-wrap items-center gap-2">
          <select aria-label="Device" className={INPUT} value={chosen} onChange={(e) => setTarget(e.target.value)} disabled={disabled}>
            {targets.map((d) => (
              <option key={d.deviceId} value={d.deviceId}>
                {d.name}
              </option>
            ))}
          </select>
          <input aria-label="Task" className={`${INPUT} min-w-0 flex-1`} placeholder="What should it do?" value={text} maxLength={2000} disabled={disabled} onChange={(e) => setText(e.target.value)} />
          <button
            type="button"
            className={PRIMARY}
            disabled={disabled || busy || !chosen || !text.trim()}
            onClick={() => void sendTask(chosen, text.trim()).then((t) => t && setText(""))}
          >
            Send
          </button>
        </div>
      )}
      {overview.tasks.length === 0 ? (
        <p className="text-sm text-muted">Nothing sent or received yet. You can also just ask IGRIS, e.g. "open Notepad on my PC".</p>
      ) : (
        <ul className="space-y-1.5">
          {overview.tasks.slice(0, 10).map((t) => (
            <li key={t.requestId} className="rounded-lg border border-line px-3 py-2">
              <div className="flex items-start gap-2">
                <span className="mt-1.5">
                  <StatusDot tone={toneDot(taskTone(t))} pulse={isActive(t)} />
                </span>
                <div className="min-w-0 flex-1">
                  <div className="text-sm break-words text-fg">{t.objective}</div>
                  <div className="text-xs text-muted">{taskStatusText(t)}</div>
                  {t.detail && !t.detail.startsWith(taskStatusText(t)) && <div className="mt-0.5 text-xs break-words text-faint">{t.detail}</div>}
                </div>
                {t.direction === "outgoing" && isActive(t) && (
                  <div className="flex gap-1">
                    {t.status === "paused" ? (
                      <button type="button" className={BUTTON} aria-label="Resume" onClick={() => void control(t.requestId, "resume")}>
                        <Play className="size-3.5" />
                      </button>
                    ) : (
                      <button type="button" className={BUTTON} aria-label="Pause" onClick={() => void control(t.requestId, "pause")}>
                        <Pause className="size-3.5" />
                      </button>
                    )}
                    <button type="button" className={BUTTON} aria-label="Stop" onClick={() => void control(t.requestId, "stop")}>
                      <Square className="size-3.5" />
                    </button>
                  </div>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
