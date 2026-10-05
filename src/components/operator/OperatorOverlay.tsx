import { Pause, Play, Square } from "lucide-react";
import { useEffect } from "react";
import { AiCore } from "@/components/core/AiCore";
import { api } from "@/services/api";
import { phaseToCore, useOperatorStore } from "@/stores/operatorStore";
import "./operator.css";

/** Apply the user's theme/accent to an overlay window (it has no app shell). */
function useOverlayTheme() {
  useEffect(() => {
    const root = document.documentElement;
    root.classList.add("operator-window");
    root.dataset.theme = "dark";
    root.dataset.accent = "azure";
    void api
      .getSettings()
      .then((s) => {
        root.dataset.accent = s.accent;
        root.dataset.reducedMotion = String(s.reducedMotion);
      })
      .catch(() => undefined);
  }, []);
}

/** Click-through animated frame around the display IGRIS is working on. */
export function OperatorBorder() {
  useOverlayTheme();
  const connect = useOperatorStore((s) => s.connect);
  useEffect(() => connect(), [connect]);
  const snap = useOperatorStore((s) => s.snapshot);
  const phase = snap.task?.phase ?? "executing";
  const visible = snap.active || (snap.task !== null && ["success", "error", "stopped"].includes(phase));

  return (
    <div className="op-border" data-phase={phase} data-visible={visible} aria-hidden="true">
      <div className="op-border__glow" />
      <div className="op-border__frame">
        <div className="op-border__base" />
        <div className="op-border__sweep" />
      </div>
    </div>
  );
}

const PHASE_LABEL: Record<string, string> = {
  planning: "PLANNING",
  verifying: "VERIFYING",
  waiting: "WAITING",
  paused: "PAUSED",
  success: "COMPLETE",
  error: "FAILED",
  stopped: "STOPPED",
};

/** The floating IGRIS orb: state, short status, Pause/Resume and Stop. Draggable. */
export function OperatorOrb() {
  useOverlayTheme();
  const connect = useOperatorStore((s) => s.connect);
  useEffect(() => connect(), [connect]);
  const snap = useOperatorStore((s) => s.snapshot);
  const control = useOperatorStore((s) => s.control);
  const task = snap.task;
  const phase = task?.phase ?? "executing";
  const paused = task?.state === "paused";
  const visible = snap.active || (task !== null && ["success", "error", "stopped"].includes(phase));
  const status = (phase === "executing" ? task?.status : PHASE_LABEL[phase]) || task?.status || "EXECUTING";

  return (
    <div className="op-orb" data-phase={phase} data-visible={visible} data-tauri-drag-region role="status" aria-live="polite">
      <div className="op-orb__core">
        <AiCore state={phaseToCore(phase)} size={58} />
      </div>
      <div className="op-orb__text" title={task?.pauseReason ?? task?.objective ?? ""}>
        <div className="op-orb__name">IGRIS</div>
        <div className="op-orb__status">{status}</div>
      </div>
      {snap.active && (
        <>
          <button
            type="button"
            className="op-orb__btn"
            aria-label={paused ? "Resume" : "Pause"}
            title={paused ? "Resume" : "Pause"}
            onClick={() => void control(paused ? "resume" : "pause")}
          >
            {paused ? <Play className="size-3.5" /> : <Pause className="size-3.5" />}
          </button>
          <button type="button" className="op-orb__btn op-orb__btn--stop" aria-label="Stop" title="Stop (Esc)" onClick={() => void control("stop")}>
            <Square className="size-3 fill-current" />
          </button>
        </>
      )}
    </div>
  );
}
