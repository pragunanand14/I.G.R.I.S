import { listen } from "@tauri-apps/api/event";
import { AlarmClock, BellRing, Timer, X } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "@/services/api";
import { BackendError, hasBackend } from "@/services/backend";
import { playChime } from "@/services/chime";
import { useAlertStore } from "@/stores/alertStore";
import type { FiredReminder, Reminder } from "@/types/productivity";
import { formatWhen } from "@/utils/time";

/** Same rule as the backend: more than a minute late means IGRIS wasn't running at the due time. */
function firedLate(r: Reminder): boolean {
  return r.firedAt !== null && new Date(r.firedAt).getTime() - new Date(r.dueAt).getTime() > 60_000;
}

/** Ringing reminders and timers, on top of every page. */
export function ReminderAlerts({ enabled }: { enabled: boolean }) {
  const ringing = useAlertStore((s) => s.ringing);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!enabled || !hasBackend()) return;
    let unlisten: (() => void) | undefined;
    let live = true;
    void listen<FiredReminder>("reminder-fired", (e) => {
      useAlertStore.getState().add(e.payload);
      playChime();
    }).then((u) => {
      if (live) unlisten = u;
      else u();
    });
    // Anything that went off before the window was listening (e.g. right at startup).
    void api
      .listReminders()
      .then((list) => list.filter((r) => r.status === "fired").forEach((r) => useAlertStore.getState().add({ ...r, late: firedLate(r) })))
      .catch(() => undefined);
    return () => {
      live = false;
      unlisten?.();
    };
  }, [enabled]);

  if (ringing.length === 0) return null;

  const act = async (id: number, fn: () => Promise<unknown>) => {
    try {
      await fn();
      setError(null);
    } catch (err) {
      setError(BackendError.from(err).message);
    } finally {
      useAlertStore.getState().remove(id);
    }
  };

  return (
    <div className="pointer-events-none fixed top-12 right-4 z-50 flex w-80 flex-col gap-2" aria-live="assertive">
      {error && <p className="pointer-events-auto rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">{error}</p>}
      {ringing.map((r) => (
        <div key={r.id} role="alert" className="pointer-events-auto rounded-xl border border-accent/40 bg-elevated p-3 shadow-xl backdrop-blur">
          <div className="flex items-start gap-2.5">
            <span className="mt-0.5 grid size-7 shrink-0 animate-pulse place-items-center rounded-full bg-accent/15 text-accent motion-reduce:animate-none">
              {r.kind === "timer" ? <Timer className="size-4" /> : r.late ? <AlarmClock className="size-4" /> : <BellRing className="size-4" />}
            </span>
            <div className="min-w-0 flex-1">
              <div className="text-[10px] font-semibold tracking-wider text-accent uppercase">
                {r.kind === "timer" ? "Timer finished" : r.late ? "Missed reminder" : "Reminder"}
              </div>
              <div className="text-sm break-words text-fg">{r.title}</div>
              {r.late && <div className="mt-0.5 text-[11px] text-faint">Was due {formatWhen(r.dueAt)} while IGRIS was closed</div>}
            </div>
            <button type="button" aria-label="Dismiss" onClick={() => void act(r.id, () => api.dismissReminder(r.id))} className="rounded-md p-1 text-faint hover:text-fg">
              <X className="size-3.5" />
            </button>
          </div>
          <div className="mt-2.5 flex gap-1.5 pl-9">
            {[5, 10].map((m) => (
              <button
                key={m}
                type="button"
                onClick={() => void act(r.id, () => api.snoozeReminder(r.id, m))}
                className="rounded-md border border-line px-2 py-1 text-[11px] text-muted hover:border-accent hover:text-accent"
              >
                Snooze {m} min
              </button>
            ))}
            <button type="button" onClick={() => void act(r.id, () => api.dismissReminder(r.id))} className="ml-auto rounded-md bg-accent px-2.5 py-1 text-[11px] font-semibold text-bg">
              Done
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}
