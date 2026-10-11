import { Sunrise, Volume2 } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "@/services/api";
import { hasBackend } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import { useVoiceStore } from "@/stores/voiceStore";
import type { RoutineRun } from "@/types/routines";

/** Today's briefing on Home, once it has run: the text and Listen. */
export function BriefingCard({ phone = false }: { phone?: boolean }) {
  const ready = useAppStore((s) => s.backend) === "ready";
  const [run, setRun] = useState<RoutineRun | null>(null);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (!ready || !hasBackend()) return;
    let live = true;
    const load = () =>
      api
        .briefingToday()
        .then((r) => live && setRun(r))
        .catch(() => undefined);
    load();
    const t = setInterval(load, 60_000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [ready]);

  if (!run?.summary) return null;
  const text = run.summary;
  const short = text.length > 160 && !open;

  return (
    <div
      className={`anim-fade mx-auto w-full max-w-md rounded-2xl px-4 py-3 text-left ${phone ? "" : "bg-surface"}`}
      style={phone ? { background: "var(--m-surface)" } : undefined}
      aria-label="Today's briefing"
    >
      <div className="flex items-center gap-2">
        <Sunrise className="size-4 shrink-0 text-accent" aria-hidden="true" />
        <span className={phone ? "flex-1 text-[14px] font-semibold" : "flex-1 text-xs font-semibold text-fg"}>Today’s briefing</span>
        <button
          type="button"
          aria-label="Listen to the briefing"
          onClick={() => void useVoiceStore.getState().say(text)}
          className={phone ? "m-icon-button" : "grid size-7 place-items-center rounded-full text-muted hover:bg-surface-hover hover:text-fg"}
        >
          <Volume2 className="size-4" />
        </button>
      </div>
      <p className={`mt-1 break-words ${phone ? "text-[15px]" : "text-sm text-muted"}`}>
        {short ? `${text.slice(0, 160)}…` : text}{" "}
        {text.length > 160 && (
          <button type="button" className={phone ? "m-link" : "text-accent"} onClick={() => setOpen(!open)}>
            {open ? "Less" : "More"}
          </button>
        )}
      </p>
    </div>
  );
}
