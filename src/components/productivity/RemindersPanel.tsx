import { BellRing, Clock, Timer, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useNow } from "@/hooks/useNow";
import { Panel } from "@/components/ui/Panel";
import { useTimePreview } from "@/hooks/useTimePreview";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAlertStore } from "@/stores/alertStore";
import type { Reminder } from "@/types/productivity";
import { formatCountdown, formatRelative, formatWhen } from "@/utils/time";
import { INPUT } from "./styles";
import { TimeHint } from "./TimeHint";

const QUICK_TIMERS = ["5 minutes", "10 minutes", "25 minutes", "1 hour"];

function ActiveRow({ r, now, onCancel }: { r: Reminder; now: number; onCancel: () => void }) {
  const due = new Date(r.dueAt).getTime();
  const left = due - now;
  const ringing = r.status === "fired";
  // Snoozed timers run longer than their original length, so a progress bar would be meaningless.
  const progress = r.kind === "timer" && r.durationSecs && left <= r.durationSecs * 1000 ? Math.min(100, Math.max(0, 100 - (left / (r.durationSecs * 1000)) * 100)) : null;
  return (
    <li className="py-2.5">
      <div className="flex items-center gap-3">
        {r.kind === "timer" ? <Timer className="size-4 shrink-0 text-accent" /> : <BellRing className="size-4 shrink-0 text-accent" />}
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm text-fg">{r.title}</div>
          <div className="text-[11px] text-faint">{ringing ? "Ringing now" : `${formatWhen(r.dueAt)} · ${formatRelative(left)}`}</div>
        </div>
        {r.kind === "timer" && !ringing && <span className="font-mono text-lg text-fg tabular-nums">{formatCountdown(left)}</span>}
        <button type="button" aria-label={`${ringing ? "Dismiss" : "Cancel"} ${r.title}`} onClick={onCancel} className="grid size-7 place-items-center rounded-md text-faint hover:text-danger">
          <X className="size-3.5" />
        </button>
      </div>
      {progress !== null && !ringing && (
        <div className="mt-2 ml-7 h-1 overflow-hidden rounded-full bg-surface-strong">
          <div className="h-full bg-accent transition-[width] duration-1000 ease-linear" style={{ width: `${progress}%` }} />
        </div>
      )}
    </li>
  );
}

export function RemindersPanel({ onError }: { onError: (msg: string | null) => void }) {
  const revision = useAlertStore((s) => s.revision);
  const [active, setActive] = useState<Reminder[]>([]);
  const [history, setHistory] = useState<Reminder[]>([]);
  const [reload, setReload] = useState(0);
  const [text, setText] = useState("");
  const [when, setWhen] = useState("");
  const [timer, setTimer] = useState("");
  const [label, setLabel] = useState("");
  const preview = useTimePreview(when);
  const hasTimer = active.some((r) => r.kind === "timer" && r.status === "pending");
  const now = useNow(hasTimer ? 1000 : 30_000);

  useEffect(() => {
    let live = true;
    Promise.all([api.listReminders(), api.reminderHistory()])
      .then(([a, h]) => {
        if (!live) return;
        setActive(a);
        setHistory(h);
      })
      .catch((err) => live && onError(BackendError.from(err).message));
    return () => {
      live = false;
    };
  }, [reload, revision, onError]);

  const run = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      onError(null);
      return true;
    } catch (err) {
      onError(BackendError.from(err).message);
      return false;
    } finally {
      setReload((k) => k + 1);
    }
  };

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Panel title="Remind me">
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void run(() => api.addReminder(text, when)).then((ok) => {
                if (ok) {
                  setText("");
                  setWhen("");
                }
              });
            }}
            className="space-y-2"
          >
            <input aria-label="Reminder" value={text} onChange={(e) => setText(e.target.value)} placeholder="Call Riya about SkillTrack" className={`${INPUT} w-full`} />
            <div className="flex gap-2">
              <input aria-label="When" value={when} onChange={(e) => setWhen(e.target.value)} placeholder="in 20 minutes · tomorrow 5pm" className={`${INPUT} min-w-0 flex-1`} />
              <button type="submit" disabled={!text.trim() || preview.state !== "ok"} className="h-8 rounded-lg bg-accent px-3 text-xs font-semibold text-bg disabled:opacity-40">
                Set
              </button>
            </div>
            <TimeHint preview={preview} />
          </form>
        </Panel>

        <Panel title="Timer">
          <div className="mb-2 flex flex-wrap gap-1.5">
            {QUICK_TIMERS.map((d) => (
              <button
                key={d}
                type="button"
                onClick={() => void run(() => api.startTimer(label, d))}
                className="rounded-full border border-line px-2.5 py-1 text-xs text-fg hover:border-accent hover:text-accent"
              >
                {d.replace("minutes", "min")}
              </button>
            ))}
          </div>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void run(() => api.startTimer(label, timer)).then((ok) => {
                if (ok) {
                  setTimer("");
                  setLabel("");
                }
              });
            }}
            className="flex gap-2"
          >
            <input aria-label="Timer length" value={timer} onChange={(e) => setTimer(e.target.value)} placeholder="1h 30m" className={`${INPUT} w-28`} />
            <input aria-label="Timer label" value={label} onChange={(e) => setLabel(e.target.value)} placeholder="Label (optional)" className={`${INPUT} min-w-0 flex-1`} />
            <button type="submit" disabled={!timer.trim()} className="h-8 rounded-lg bg-accent px-3 text-xs font-semibold text-bg disabled:opacity-40">
              Start
            </button>
          </form>
        </Panel>
      </div>

      <Panel title={`Upcoming · ${active.length}`}>
        {active.length === 0 ? (
          <p className="py-1 text-sm text-faint">No reminders or timers. Try asking IGRIS: “remind me in 20 minutes to stretch”.</p>
        ) : (
          <ul className="divide-y divide-line">
            {active.map((r) => (
              <ActiveRow key={r.id} r={r} now={now} onCancel={() => void run(() => (r.status === "fired" ? api.dismissReminder(r.id) : api.cancelReminder(r.id)))} />
            ))}
          </ul>
        )}
        <p className="mt-3 flex items-center gap-1 text-[11px] text-faint">
          <Clock className="size-3" /> Reminders fire while IGRIS is running (it can sit in the background). Ones missed while it was closed go off when it next starts.
        </p>
      </Panel>

      {history.length > 0 && (
        <Panel
          title="Recent"
          action={
            <button type="button" onClick={() => void run(api.clearReminderHistory)} className="text-xs text-muted hover:text-danger">
              Clear
            </button>
          }
        >
          <ul className="space-y-1">
            {history.slice(0, 10).map((r) => (
              <li key={r.id} className="flex items-center gap-2 text-xs text-faint">
                <span className="min-w-0 flex-1 truncate">{r.title}</span>
                <span>{formatWhen(r.dueAt)}</span>
                <span className="w-16 text-right">{r.status}</span>
              </li>
            ))}
          </ul>
        </Panel>
      )}
    </div>
  );
}
