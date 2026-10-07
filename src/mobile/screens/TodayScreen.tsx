import { Bell, CalendarDays, Check, Circle, Timer, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useState, type FormEvent } from "react";
import { useTimePreview } from "@/hooks/useTimePreview";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import type { CalendarEvent, Reminder, Task } from "@/types/productivity";
import { whenText } from "../format";
import { Screen, Section } from "../ui";

interface Day {
  reminders: Reminder[];
  tasks: Task[];
  events: CalendarEvent[];
}

async function fetchDay(): Promise<Day> {
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  const end = new Date(start.getTime() + 86400000);
  const [reminders, tasks, events] = await Promise.all([api.listReminders(), api.listTasks("open"), api.listEvents(start.toISOString(), end.toISOString())]);
  return { reminders: reminders.filter((r) => r.status === "pending").sort((a, b) => a.dueAt.localeCompare(b.dueAt)), tasks, events };
}

/** Reminders, timers, to-dos and today's events in one list. */
export function TodayScreen() {
  const backend = useAppStore((s) => s.backend);
  const [day, setDay] = useState<Day | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(
    () =>
      fetchDay()
        .then((d) => {
          setDay(d);
          setError(null);
        })
        .catch((err) => setError(BackendError.from(err).message)),
    [],
  );

  useEffect(() => {
    if (backend !== "ready") return;
    let live = true;
    const refresh = () =>
      fetchDay()
        .then((d) => live && setDay(d))
        .catch((err) => live && setError(BackendError.from(err).message));
    void refresh();
    const id = setInterval(() => void refresh(), 30000);
    return () => {
      live = false;
      clearInterval(id);
    };
  }, [backend]);

  const act = async (f: () => Promise<unknown>) => {
    try {
      await f();
    } catch (err) {
      setError(BackendError.from(err).message);
    }
    await load();
  };

  return (
    <Screen title="Today" subtitle={new Date().toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" })}>
      {backend !== "ready" ? (
        <p className="m-muted">Your reminders and to-dos appear here once IGRIS is running.</p>
      ) : (
        <>
          <AddBox onAdded={load} onError={setError} />
          {error && (
            <p role="alert" className="text-sm" style={{ color: "var(--m-bad)" }}>
              {error}
            </p>
          )}

          <Section title="Reminders and timers">
            <div className="m-card">
              {day?.reminders.length === 0 && <p className="m-row m-muted">Nothing scheduled.</p>}
              {day?.reminders.map((r) => (
                <div key={r.id} className="m-row">
                  <span className="m-icon-badge">{r.kind === "timer" ? <Timer className="size-5" /> : <Bell className="size-5" />}</span>
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-medium">{r.title}</div>
                    <div className="m-muted text-sm">{whenText(r.dueAt)}</div>
                  </div>
                  <button type="button" className="m-faint grid size-11 place-items-center" aria-label={`Cancel ${r.title}`} onClick={() => void act(() => api.cancelReminder(r.id))}>
                    <X className="size-5" />
                  </button>
                </div>
              ))}
            </div>
          </Section>

          <Section title="To-do">
            <div className="m-card">
              {day?.tasks.length === 0 && <p className="m-row m-muted">Your list is empty.</p>}
              {day?.tasks.map((t) => (
                <div key={t.id} className="m-row">
                  <button
                    type="button"
                    className="grid size-11 shrink-0 place-items-center"
                    aria-label={`Mark “${t.title}” done`}
                    onClick={() => void act(() => api.setTaskDone(t.id, true))}
                  >
                    <Circle className="m-faint size-6" />
                  </button>
                  <div className="min-w-0 flex-1">
                    <div className="font-medium">{t.title}</div>
                    {t.dueAt && <div className="m-muted text-sm">Due {whenText(t.dueAt)}</div>}
                  </div>
                  <button type="button" className="m-faint grid size-11 place-items-center" aria-label={`Delete ${t.title}`} onClick={() => void act(() => api.deleteTask(t.id))}>
                    <Trash2 className="size-5" />
                  </button>
                </div>
              ))}
            </div>
          </Section>

          {day && day.events.length > 0 && (
            <Section title="Events today">
              <div className="m-card">
                {day.events.map((e) => (
                  <div key={e.id} className="m-row">
                    <span className="m-icon-badge">
                      <CalendarDays className="size-5" />
                    </span>
                    <div className="min-w-0 flex-1">
                      <div className="truncate font-medium">{e.title}</div>
                      <div className="m-muted text-sm">
                        {e.allDay ? "All day" : new Date(e.startsAt).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}
                        {e.location ? ` · ${e.location}` : ""}
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </Section>
          )}
        </>
      )}
    </Screen>
  );
}

/** One box: what, and optionally when. A reminder needs a time; a to-do doesn't. */
function AddBox({ onAdded, onError }: { onAdded: () => Promise<void>; onError: (m: string | null) => void }) {
  const [what, setWhat] = useState("");
  const [when, setWhen] = useState("");
  const [busy, setBusy] = useState(false);
  const preview = useTimePreview(when);

  const run = async (kind: "reminder" | "todo") => {
    const title = what.trim();
    if (!title) return;
    setBusy(true);
    onError(null);
    try {
      if (kind === "reminder") await api.addReminder(title, when.trim());
      else await api.addTask({ title, notes: "", dueAt: preview.state === "ok" ? preview.parsed.at : "", priority: "normal" });
      setWhat("");
      setWhen("");
      await onAdded();
    } catch (err) {
      onError(BackendError.from(err).message);
    } finally {
      setBusy(false);
    }
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    void run(when.trim() ? "reminder" : "todo");
  };

  return (
    <form onSubmit={submit} className="m-card space-y-3 p-4">
      <input className="m-input" value={what} onChange={(e) => setWhat(e.target.value)} placeholder="What do you need to remember?" aria-label="What" />
      <input
        className="m-input"
        value={when}
        onChange={(e) => setWhen(e.target.value)}
        placeholder="When? e.g. in 20 min, 6pm"
        aria-label="When"
      />
      <p className="m-muted min-h-5 px-1 text-sm" aria-live="polite">
        {preview.state === "ok" && (
          <span className="inline-flex items-center gap-1">
            <Check className="m-accent size-4" /> {preview.parsed.description}
          </span>
        )}
        {preview.state === "error" && <span style={{ color: "var(--m-warn)" }}>{preview.message}</span>}
        {preview.state === "empty" && "Leave the time empty to just add it to your to-do list."}
      </p>
      <div className="grid grid-cols-2 gap-2">
        <button type="button" className="m-button m-button-primary" disabled={busy || !what.trim() || preview.state !== "ok"} onClick={() => void run("reminder")}>
          <Bell className="size-5" /> Remind me
        </button>
        <button type="button" className="m-button" disabled={busy || !what.trim()} onClick={() => void run("todo")}>
          <Check className="size-5" /> Add to-do
        </button>
      </div>
    </form>
  );
}
