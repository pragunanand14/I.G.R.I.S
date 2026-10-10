import {
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  CircleDot,
  FastForward,
  House,
  Loader2,
  Menu,
  Minus,
  Pause,
  Play,
  Plus,
  Power,
  Rewind,
  Search,
  Tv,
  Undo2,
  Volume2,
  VolumeX,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import type { FoundDevice, HomeDevice, TvActionName, TvState } from "@/types/home";
import { Touchpad } from "./Touchpad";

const SURFACE = "rounded-[20px] bg-surface";
const PILL = "rounded-full bg-surface-strong px-4 py-2 text-sm font-medium text-fg hover:bg-surface-hover disabled:opacity-40";
const PRIMARY = "rounded-full bg-fg px-4 py-2 text-sm font-semibold text-bg disabled:opacity-40";

/**
 * Smart home: add the TV (it asks for permission on its screen), then a
 * remote — power, volume, channels, arrows and OK, media, apps and inputs.
 * Every press reports what the TV really did (or why it couldn't).
 */
export function SmartHome({ phone = false }: { phone?: boolean }) {
  const ready = useAppStore((s) => s.backend) === "ready";
  const [devices, setDevices] = useState<HomeDevice[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);

  const [version, setVersion] = useState(0);
  const load = useCallback(() => setVersion((v) => v + 1), []);
  useEffect(() => {
    if (!ready) return;
    let live = true;
    api.homeDevices().then(
      (d) => {
        if (!live) return;
        setDevices(d);
        setSelected((s) => (s && d.some((x) => x.id === s) ? s : (d[0]?.id ?? null)));
      },
      (err) => live && setError(BackendError.from(err).message),
    );
    return () => {
      live = false;
    };
  }, [ready, version]);

  if (!ready) return <p className="text-sm text-muted">Smart home is available once IGRIS is running.</p>;
  if (devices === null) return <p className="text-sm text-muted">{error ?? "Loading…"}</p>;
  const device = devices.find((d) => d.id === selected) ?? null;

  return (
    <div className="space-y-6">
      {error && (
        <p role="alert" className="text-sm text-danger">
          {error}
        </p>
      )}
      {devices.length > 1 && (
        <div className="flex flex-wrap gap-2" role="tablist" aria-label="Devices">
          {devices.map((d) => (
            <button key={d.id} type="button" role="tab" aria-selected={d.id === selected} onClick={() => setSelected(d.id)} className={d.id === selected ? PRIMARY : PILL}>
              {d.name}
            </button>
          ))}
        </div>
      )}
      {device && !adding && <Remote key={device.id} device={device} phone={phone} onChanged={load} />}
      {(devices.length === 0 || adding) && (
        <AddTv
          onAdded={(d) => {
            setAdding(false);
            setSelected(d.id);
            load();
          }}
          onCancel={devices.length > 0 ? () => setAdding(false) : undefined}
        />
      )}
      {devices.length > 0 && !adding && (
        <button type="button" className="text-sm text-muted underline-offset-2 hover:text-fg hover:underline" onClick={() => setAdding(true)}>
          Add another TV
        </button>
      )}
      <p className="text-xs text-faint">
        IGRIS talks to your TV directly over your home Wi-Fi — no other app or cloud. Today it supports LG TVs (webOS); more devices can be added the same
        way.
      </p>
    </div>
  );
}

/** Find the TV on the network (or type its address), then pair: the TV asks for permission on screen. */
function AddTv({ onAdded, onCancel }: { onAdded: (d: HomeDevice) => void; onCancel?: () => void }) {
  const [found, setFound] = useState<FoundDevice[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [host, setHost] = useState("");
  const [pairing, setPairing] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const search = async () => {
    setSearching(true);
    setError(null);
    try {
      setFound(await api.homeDiscover());
    } catch (err) {
      setError(BackendError.from(err).message);
    } finally {
      setSearching(false);
    }
  };
  const pair = async (address: string, name?: string) => {
    setPairing(address);
    setError(null);
    try {
      onAdded(await api.homePair(address, name));
    } catch (err) {
      setError(BackendError.from(err).message);
    } finally {
      setPairing(null);
    }
  };

  return (
    <section className={`anim-rise ${SURFACE} space-y-5 p-6`}>
      <div className="flex items-start gap-4">
        <span className="grid size-11 shrink-0 place-items-center rounded-2xl bg-surface-strong text-muted">
          <Tv className="size-5" />
        </span>
        <div className="min-w-0">
          <h2 className="text-[15px] font-semibold text-fg">Add your LG TV</h2>
          <p className="mt-1 text-sm text-muted">Turn the TV on and make sure it's on the same Wi-Fi as this device.</p>
        </div>
      </div>

      {pairing ? (
        <div className="anim-fade flex items-center gap-3 rounded-2xl bg-surface-strong p-4" role="status">
          <Loader2 className="size-5 shrink-0 animate-spin text-accent" />
          <div className="text-sm">
            <p className="font-medium text-fg">Look at your TV and press Allow.</p>
            <p className="text-muted">It's asking whether IGRIS may control it. Waiting up to a minute…</p>
          </div>
        </div>
      ) : (
        <>
          <button type="button" className={`${PRIMARY} inline-flex items-center gap-2`} disabled={searching} onClick={() => void search()}>
            {searching ? <Loader2 className="size-4 animate-spin" /> : <Search className="size-4" />} {searching ? "Looking for TVs…" : "Find my TV"}
          </button>
          {found && (
            <ul className="space-y-2" aria-label="TVs found">
              {found.length === 0 && <li className="text-sm text-muted">No LG TV answered. Check it's on and on this Wi-Fi, or enter its address below.</li>}
              {found.map((f) => (
                <li key={f.host} className="anim-rise flex items-center justify-between gap-3 rounded-2xl bg-surface-strong px-4 py-3">
                  <span className="min-w-0">
                    <span className="block truncate text-sm font-medium text-fg">{f.name}</span>
                    <span className="block text-xs text-faint">{f.host}</span>
                  </span>
                  <button type="button" className={PRIMARY} onClick={() => void pair(f.host, f.name)}>
                    Pair
                  </button>
                </li>
              ))}
            </ul>
          )}
          <form
            className="flex flex-wrap items-center gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (host.trim()) void pair(host.trim());
            }}
          >
            <input
              aria-label="TV IP address"
              inputMode="decimal"
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              placeholder="Or its IP address, e.g. 192.168.1.20"
              value={host}
              onChange={(e) => setHost(e.target.value)}
              className="h-10 min-w-0 flex-1 rounded-xl bg-surface-strong px-3 text-sm text-fg transition-shadow duration-200 placeholder:text-faint focus:ring-2 focus:ring-accent/50 focus:outline-none"
            />
            <button type="submit" className={PILL} disabled={!host.trim()}>
              Pair
            </button>
          </form>
          <p className="text-xs text-faint">The address is on the TV: Settings → Network → Wi-Fi connection → Advanced settings.</p>
        </>
      )}
      {error && (
        <p role="alert" className="text-sm text-danger">
          {error}
        </p>
      )}
      {onCancel && !pairing && (
        <button type="button" className="text-sm text-muted hover:text-fg" onClick={onCancel}>
          Cancel
        </button>
      )}
    </section>
  );
}

/** The remote for one TV. */
function Remote({ device, phone, onChanged }: { device: HomeDevice; phone: boolean; onChanged: () => void }) {
  const [state, setState] = useState<TvState | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const noteTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const [version, setVersion] = useState(0);
  const refresh = useCallback(() => setVersion((v) => v + 1), []);
  useEffect(() => {
    let live = true;
    api.homeTvState(device.id).then(
      (s) => live && setState(s),
      () => live && setState({ on: false, volume: null, muted: null, app: null, apps: [], inputs: [] }),
    );
    return () => {
      live = false;
    };
  }, [device.id, version]);

  const press = async (key: string, action: TvActionName, value?: string | number, after?: () => void) => {
    setBusy(key);
    clearTimeout(noteTimer.current);
    try {
      const text = await api.homeTv(device.id, action, value);
      setNote({ ok: true, text });
      after?.();
    } catch (err) {
      setNote({ ok: false, text: BackendError.from(err).message });
    } finally {
      setBusy(null);
      noteTimer.current = setTimeout(() => setNote(null), 4000);
    }
  };
  const on = state?.on ?? false;
  const btn = (key: string, label: string, icon: ReactNode, action: TvActionName, value?: string | number, extra = "") => (
    <RemoteButton label={label} busy={busy === key} disabled={!on || busy !== null} onClick={() => void press(key, action, value)} className={extra}>
      {icon}
    </RemoteButton>
  );

  return (
    <section className={`anim-rise ${SURFACE} space-y-6 p-6`} aria-label={`${device.name} remote`}>
      <header className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <h2 className="truncate text-[17px] font-semibold text-fg">{device.name}</h2>
          <p className="mt-0.5 flex items-center gap-1.5 text-sm text-muted" role="status">
            <span className={`inline-block size-2 rounded-full ${state === null ? "bg-faint" : on ? "bg-success" : "bg-warning"}`} />
            {state === null ? "Checking…" : on ? `On${state.volume !== null ? ` · volume ${state.volume}${state.muted ? " (muted)" : ""}` : ""}` : "Off or not reachable"}
          </p>
        </div>
        <RemoteButton
          label={on ? "Turn off" : "Turn on"}
          busy={busy === "power"}
          disabled={busy !== null || (!on && !device.canPowerOn) || state === null}
          onClick={() => void press("power", on ? "power_off" : "power_on", undefined, () => setTimeout(refresh, on ? 1500 : 300))}
          className={on ? "bg-danger/15 text-danger" : ""}
        >
          <Power className="size-5" />
        </RemoteButton>
      </header>

      {!on && state !== null && (
        <p className="text-sm text-muted">
          {device.canPowerOn
            ? 'Press power to switch it on (the TV needs "Turn on via Wi-Fi" enabled).'
            : "Turn the TV on with its remote; IGRIS can't switch this one on yet."}{" "}
          <button type="button" className="text-fg underline-offset-2 hover:underline" onClick={refresh}>
            Check again
          </button>
        </p>
      )}

      <div className={`grid gap-6 transition-opacity duration-300 sm:grid-cols-[auto_1fr] ${on ? "" : "pointer-events-none opacity-40"}`}>
        {/* Arrows and OK */}
        <div className="mx-auto grid w-[188px] grid-cols-3 gap-2">
          <span />
          {btn("up", "Up", <ChevronUp className="size-5" />, "button", "UP")}
          <span />
          {btn("left", "Left", <ChevronLeft className="size-5" />, "button", "LEFT")}
          {btn("ok", "OK", <CircleDot className="size-5" />, "button", "ENTER", "bg-surface-hover")}
          {btn("right", "Right", <ChevronRight className="size-5" />, "button", "RIGHT")}
          {btn("back", "Back", <Undo2 className="size-5" />, "button", "BACK")}
          {btn("down", "Down", <ChevronDown className="size-5" />, "button", "DOWN")}
          {btn("home", "Home", <House className="size-5" />, "button", "HOME")}
        </div>

        <div className="space-y-4">
          <div className="flex flex-wrap items-center gap-2">
            <span className="w-full text-xs text-faint sm:w-16">Volume</span>
            {btn("vdown", "Volume down", <Minus className="size-4" />, "volume_down")}
            {btn(
              "mute",
              state?.muted ? "Unmute" : "Mute",
              state?.muted ? <VolumeX className="size-4" /> : <Volume2 className="size-4" />,
              state?.muted ? "unmute" : "mute",
            )}
            {btn("vup", "Volume up", <Plus className="size-4" />, "volume_up")}
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <span className="w-full text-xs text-faint sm:w-16">Channel</span>
            {btn("cdown", "Channel down", <ChevronDown className="size-4" />, "channel_down")}
            {btn("cup", "Channel up", <ChevronUp className="size-4" />, "channel_up")}
            {btn("menu", "Menu", <Menu className="size-4" />, "button", "MENU")}
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <span className="w-full text-xs text-faint sm:w-16">Media</span>
            {btn("rew", "Rewind", <Rewind className="size-4" />, "rewind")}
            {btn("play", "Play", <Play className="size-4" />, "play")}
            {btn("pause", "Pause", <Pause className="size-4" />, "pause")}
            {btn("ff", "Fast forward", <FastForward className="size-4" />, "fast_forward")}
          </div>
        </div>
      </div>

      {on && <Touchpad id={device.id} phone={phone} />}

      {on && state && state.apps.length > 0 && (
        <Chips
          title="Apps"
          items={state.apps.slice(0, 16)}
          current={state.app}
          busy={busy}
          onPick={(a) => void press(`app:${a.id}`, "open_app", a.title, () => setTimeout(refresh, 1500))}
        />
      )}
      {on && state && state.inputs.length > 0 && (
        <Chips title="Inputs" items={state.inputs} current={state.app} busy={busy} onPick={(a) => void press(`app:${a.id}`, "switch_input", a.title)} />
      )}

      <div className="min-h-5 text-sm" aria-live="polite">
        {note && (
          <p key={note.text} className={`anim-fade ${note.ok ? "text-muted" : "text-danger"}`}>
            {note.text}
          </p>
        )}
      </div>

      <footer className="flex items-center justify-between gap-3 border-t border-line pt-4 text-xs text-faint">
        <span>{device.host}</span>
        {confirmRemove ? (
          <span className="flex items-center gap-2">
            Remove {device.name}?
            <button type="button" className="text-muted hover:text-fg" onClick={() => setConfirmRemove(false)}>
              Keep
            </button>
            <button
              type="button"
              className="font-medium text-danger"
              onClick={() => void api.homeRemove(device.id).then(onChanged, (err) => setNote({ ok: false, text: BackendError.from(err).message }))}
            >
              Remove
            </button>
          </span>
        ) : (
          <button type="button" className="hover:text-fg" onClick={() => setConfirmRemove(true)}>
            Remove this TV
          </button>
        )}
      </footer>
    </section>
  );
}

function RemoteButton({
  label,
  busy,
  disabled,
  onClick,
  className = "",
  children,
}: {
  label: string;
  busy: boolean;
  disabled: boolean;
  onClick: () => void;
  className?: string;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
      className={`grid size-14 place-items-center rounded-2xl bg-surface-strong text-fg hover:bg-surface-hover disabled:opacity-40 ${className}`}
    >
      {busy ? <Loader2 className="size-4 animate-spin" /> : children}
    </button>
  );
}

function Chips({
  title,
  items,
  current,
  busy,
  onPick,
}: {
  title: string;
  items: { id: string; title: string }[];
  current: string | null;
  busy: string | null;
  onPick: (a: { id: string; title: string }) => void;
}) {
  return (
    <div className="space-y-2">
      <h3 className="text-xs font-medium text-faint">{title}</h3>
      <div className="flex flex-wrap gap-2">
        {items.map((a) => (
          <button
            key={a.id}
            type="button"
            disabled={busy !== null}
            onClick={() => onPick(a)}
            className={`rounded-full px-3.5 py-1.5 text-xs font-medium disabled:opacity-40 ${a.id === current ? "bg-fg text-bg" : "bg-surface-strong text-fg hover:bg-surface-hover"}`}
          >
            {busy === `app:${a.id}` ? "…" : a.title}
          </button>
        ))}
      </div>
    </div>
  );
}
