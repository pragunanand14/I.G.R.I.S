import { Pause, Play, Square } from "lucide-react";
import { useOperatorStore } from "@/stores/operatorStore";
import { useSettingsStore } from "@/stores/settingsStore";

/** Shown at the top of the main window while IGRIS operates the computer. */
export function OperatorBanner() {
  const snap = useOperatorStore((s) => s.snapshot);
  const control = useOperatorStore((s) => s.control);
  const hotkey = useSettingsStore((s) => s.settings.operatorStopHotkey);
  const task = snap.task;
  if (!snap.active || !task) return null;
  const paused = task.state === "paused";

  return (
    <div role="status" className="anim-fade mx-2 mb-2 flex shrink-0 items-center gap-3 rounded-xl bg-accent/10 px-4 py-2 text-sm" data-testid="operator-banner">
      <span className="relative flex size-2.5">
        <span className={`absolute inline-flex size-full rounded-full bg-accent opacity-60 ${paused ? "" : "animate-ping"}`} />
        <span className="relative inline-flex size-2.5 rounded-full bg-accent" />
      </span>
      <div className="min-w-0 flex-1">
        <span className="font-medium text-fg">{paused ? "Operator mode paused" : "IGRIS is operating your computer"}</span>
        <span className="ml-2 font-mono text-[11px] tracking-wider text-muted">{task.status}</span>
        <p className="truncate text-xs text-muted">
          {task.pauseReason ?? task.objective}
          {!paused && ` · Press ${hotkey === "Escape" ? "Esc" : hotkey} to stop`}
        </p>
      </div>
      <button
        type="button"
        onClick={() => void control(paused ? "resume" : "pause")}
        className="flex items-center gap-1.5 rounded-full bg-surface-strong px-3 py-1.5 text-xs font-medium text-fg hover:bg-surface-hover"
      >
        {paused ? <Play className="size-3.5" /> : <Pause className="size-3.5" />}
        {paused ? "Resume" : "Pause"}
      </button>
      <button
        type="button"
        onClick={() => void control("stop")}
        className="flex items-center gap-1.5 rounded-lg bg-danger/90 px-2.5 py-1 text-xs font-medium text-white hover:bg-danger"
      >
        <Square className="size-3 fill-current" />
        Stop
      </button>
    </div>
  );
}
