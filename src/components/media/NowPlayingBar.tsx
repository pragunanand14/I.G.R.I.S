import { Music2, Pause, Play, SkipBack, SkipForward } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "@/services/api";
import { hasBackend } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import type { MediaAction, NowPlaying } from "@/types/media";

const CHECK_EVERY_MS = 4000;

/**
 * A small bar with what's playing and play/pause/skip — shown only while a
 * player is active on this device. Hidden where music control isn't available.
 */
export function NowPlayingBar({ phone = false }: { phone?: boolean }) {
  const ready = useAppStore((s) => s.backend) === "ready";
  const [now, setNow] = useState<NowPlaying | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!ready || !hasBackend()) return;
    let live = true;
    const check = () => {
      if (document.visibilityState !== "visible") return;
      api
        .mediaNowPlaying()
        .then((n) => live && setNow(n))
        .catch(() => live && setNow(null));
    };
    check();
    const t = setInterval(check, CHECK_EVERY_MS);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [ready]);

  if (!now || !(now.active || now.playing)) return null;

  const press = (action: MediaAction) => {
    setBusy(true);
    api
      .mediaControl(action)
      .then(setNow)
      .catch(() => undefined)
      .finally(() => setBusy(false));
  };
  const what = now.title ? (now.artist ? `${now.title} · ${now.artist}` : now.title) : now.playing ? "Music is playing" : "Paused";
  const button = phone ? "m-icon-button" : "grid size-8 place-items-center rounded-full text-muted hover:bg-surface-hover hover:text-fg disabled:opacity-40";

  return (
    <div
      className={`anim-fade mx-auto flex max-w-sm items-center gap-1 rounded-full py-1 pr-1 pl-3.5 ${phone ? "" : "bg-surface"}`}
      style={phone ? { background: "var(--m-surface)" } : undefined}
      role="group"
      aria-label="Now playing"
    >
      <Music2 className={`size-4 shrink-0 ${now.playing ? "text-accent" : "text-faint"}`} aria-hidden="true" />
      <span className={`min-w-0 flex-1 truncate px-1.5 text-left ${phone ? "text-[14px]" : "text-xs text-muted"}`} title={now.app ? `${what} — ${now.app}` : what}>
        {what}
      </span>
      <button type="button" className={button} aria-label="Previous track" disabled={busy} onClick={() => press("previous")}>
        <SkipBack className="size-4" />
      </button>
      <button type="button" className={button} aria-label={now.playing ? "Pause" : "Play"} disabled={busy} onClick={() => press("play_pause")}>
        {now.playing ? <Pause className="size-4" /> : <Play className="size-4" />}
      </button>
      <button type="button" className={button} aria-label="Next track" disabled={busy} onClick={() => press("next")}>
        <SkipForward className="size-4" />
      </button>
    </div>
  );
}
