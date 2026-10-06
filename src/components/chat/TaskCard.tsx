import { Check, Circle, CircleDot, ListChecks, Loader2, Pause, Play, RotateCcw, Square, TriangleAlert, X } from "lucide-react";
import { latestTask, useTaskStore } from "@/stores/taskStore";
import { FINAL_STATES, type StepStatus, type TaskInfo } from "@/types/task";

function stateLabel(t: TaskInfo): string {
  switch (t.state) {
    case "created":
    case "planning":
      return "Planning";
    case "waiting_for_approval":
      return "Waiting for your approval";
    case "executing":
      return "Working";
    case "paused":
      return t.live ? "Paused" : t.context.interrupted ? "Interrupted" : "Paused";
    case "verifying":
      return "Checking the result";
    case "recovering":
      return "A step failed — adjusting";
    case "completed":
      return "Done · verified";
    case "failed":
      return "Failed";
    case "cancelled":
      return "Stopped";
    case "ended":
      return "Finished · not verified";
  }
}

const STEP_ICON: Record<StepStatus, JSX.Element> = {
  pending: <Circle className="size-3 text-faint" />,
  active: <CircleDot className="size-3 text-accent" />,
  completed: <Check className="size-3 text-success" />,
  failed: <TriangleAlert className="size-3 text-danger" />,
  skipped: <X className="size-3 text-faint" />,
};

interface Props {
  conversationId: string | null;
  /** Continue an interrupted or failed task (runs a new turn). */
  onResume: (taskId: string, conversationId: string) => void;
  /** A reply is streaming somewhere; resuming must wait. */
  busy: boolean;
}

/** The current task of a conversation: state, plan progress and controls. Only real progress is shown. */
export function TaskCard({ conversationId, onResume, busy }: Props) {
  const tasks = useTaskStore((s) => s.tasks);
  const hidden = useTaskStore((s) => s.hidden);
  const control = useTaskStore((s) => s.control);
  const hide = useTaskStore((s) => s.hide);
  const error = useTaskStore((s) => s.error);
  const t = latestTask(tasks, conversationId, hidden);
  if (!t) return null;

  const final = FINAL_STATES.includes(t.state);
  const waitingForUser = !t.live && (t.state === "paused" || t.state === "failed");
  const step = t.currentStep !== null ? t.plan[t.currentStep] : undefined;
  const tone = t.state === "failed" ? "border-danger/30" : t.state === "completed" ? "border-success/30" : "border-line";
  const btn = "flex items-center gap-1.5 rounded-lg border border-line px-2.5 py-1 text-xs text-fg hover:bg-surface-hover disabled:opacity-50";

  return (
    <div role="status" aria-label="Task" data-testid="task-card" className={`mb-2 rounded-xl border ${tone} bg-surface/70 px-3.5 py-2.5 text-sm`}>
      <div className="flex items-center gap-2">
        {t.live && !final && t.state !== "paused" ? <Loader2 className="size-3.5 animate-spin text-accent" /> : <ListChecks className="size-3.5 text-muted" />}
        <span className="font-mono text-[11px] uppercase tracking-wider text-muted">{stateLabel(t)}</span>
        {t.project && <span className="rounded border border-line px-1.5 text-[10px] text-faint">{t.project.name}</span>}
        <span className="min-w-0 flex-1" />
        {t.live && !final && (
          <>
            <button type="button" className={btn} onClick={() => void control(t.id, t.state === "paused" ? "resume" : "pause")}>
              {t.state === "paused" ? <Play className="size-3.5" /> : <Pause className="size-3.5" />}
              {t.state === "paused" ? "Resume" : "Pause"}
            </button>
            <button
              type="button"
              className="flex items-center gap-1.5 rounded-lg bg-danger/90 px-2.5 py-1 text-xs font-medium text-white hover:bg-danger"
              onClick={() => void control(t.id, "stop")}
            >
              <Square className="size-3 fill-current" /> Stop
            </button>
          </>
        )}
        {waitingForUser && t.conversationId && (
          <>
            <button type="button" className={btn} disabled={busy} onClick={() => onResume(t.id, t.conversationId!)}>
              {t.state === "failed" ? <RotateCcw className="size-3.5" /> : <Play className="size-3.5" />}
              {t.state === "failed" ? "Try again" : "Resume"}
            </button>
            {t.state === "paused" && (
              <button type="button" className={btn} onClick={() => void control(t.id, "stop")}>
                Dismiss
              </button>
            )}
          </>
        )}
        {final && (
          <button type="button" aria-label="Close" className="text-faint hover:text-fg" onClick={() => hide(t.id)}>
            <X className="size-3.5" />
          </button>
        )}
      </div>
      <p className="mt-1 truncate text-fg" title={t.objective}>
        {t.objective}
      </p>
      {step && !final && (
        <p className="mt-0.5 text-xs text-muted">
          Step {t.currentStep! + 1} of {t.plan.length} · {step.title}
        </p>
      )}
      {t.plan.length > 1 && (
        <ol className="mt-1.5 space-y-0.5" aria-label="Plan">
          {t.plan.map((s, i) => (
            <li key={i} className="flex items-center gap-1.5 text-xs text-muted">
              {STEP_ICON[s.status]}
              <span className={s.status === "completed" || s.status === "skipped" ? "line-through opacity-70" : ""}>{s.title}</span>
            </li>
          ))}
        </ol>
      )}
      {t.live && t.activity && !final && <p className="mt-1 truncate font-mono text-[11px] text-faint">{t.activity}</p>}
      {t.pauseReason && t.state === "paused" && <p className="mt-1 text-xs text-warning">{t.pauseReason}</p>}
      {t.failures > 0 && !final && <p className="mt-1 text-xs text-warning">{t.failures === 1 ? "1 step failed" : `${t.failures} steps failed`} so far</p>}
      {error && <p className="mt-1 text-xs text-danger">{error}</p>}
      {final && (t.error ?? t.result) && <p className={`mt-1 text-xs ${t.state === "failed" ? "text-danger" : "text-muted"}`}>{t.error ?? t.result}</p>}
    </div>
  );
}
