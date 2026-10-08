import { ChevronLeft, ChevronRight, MapPin, Pencil, Plus, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Panel } from "@/components/ui/Panel";
import { Toggle } from "@/components/ui/Toggle";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import type { CalendarEvent } from "@/types/productivity";
import { addDays, dayDiff, formatClock, formatDay, startOfDay } from "@/utils/time";
import { draftOf, draftToInput, newDraft, type Draft } from "./eventDraft";
import { INPUT } from "./styles";

const SPAN_DAYS = 14;

function EventForm({ draft, onCancel, onSaved, onError }: { draft: Draft; onCancel: () => void; onSaved: () => void; onError: (m: string | null) => void }) {
  const [d, setD] = useState(draft);
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setD((x) => ({ ...x, [k]: v }));
  const save = async () => {
    const input = draftToInput(d);
    if (typeof input === "string") return onError(input);
    try {
      if (d.id === null) await api.addEvent(input);
      else await api.updateEvent(d.id, input);
      onError(null);
      onSaved();
    } catch (err) {
      onError(BackendError.from(err).message);
    }
  };
  return (
    <Panel title={d.id === null ? "New event" : "Edit event"} className="mb-4">
      <div className="space-y-2">
        <input aria-label="Event title" value={d.title} onChange={(e) => set("title", e.target.value)} placeholder="Team review" className={`${INPUT} w-full`} />
        <div className="flex flex-wrap items-center gap-2">
          <input aria-label="Date" type="date" value={d.date} onChange={(e) => set("date", e.target.value)} className={INPUT} />
          {!d.allDay && (
            <>
              <input aria-label="Start time" type="time" value={d.start} onChange={(e) => set("start", e.target.value)} className={INPUT} />
              <span className="text-xs text-faint">to</span>
              <input aria-label="End time" type="time" value={d.end} onChange={(e) => set("end", e.target.value)} className={INPUT} />
            </>
          )}
          <label className="ml-auto flex items-center gap-2 text-xs text-muted">
            All day <Toggle label="All day" checked={d.allDay} onChange={(v) => set("allDay", v)} />
          </label>
        </div>
        <input aria-label="Location" value={d.location} onChange={(e) => set("location", e.target.value)} placeholder="Location (optional)" className={`${INPUT} w-full`} />
        <textarea aria-label="Event notes" value={d.notes} onChange={(e) => set("notes", e.target.value)} rows={2} placeholder="Notes" className={`${INPUT} h-auto w-full resize-y py-1.5`} />
        <div className="flex justify-end gap-2">
          <button type="button" onClick={onCancel} className="rounded-lg px-3 py-1.5 text-xs text-muted hover:bg-surface-hover">
            Cancel
          </button>
          <button type="button" disabled={!d.title.trim() || !d.date} onClick={() => void save()} className="rounded-lg bg-accent px-3 py-1.5 text-xs font-semibold text-bg disabled:opacity-40">
            Save event
          </button>
        </div>
      </div>
    </Panel>
  );
}

function eventTime(e: CalendarEvent, day: Date): string {
  if (e.allDay) return "All day";
  const s = new Date(e.startsAt);
  const en = e.endsAt ? new Date(e.endsAt) : null;
  const startsToday = dayDiff(s, day) === 0;
  const start = startsToday ? formatClock(s) : "…";
  if (!en) return start;
  return `${start}–${dayDiff(en, day) === 0 ? formatClock(en) : "…"}`;
}

export function CalendarPanel({ onError }: { onError: (msg: string | null) => void }) {
  const [from, setFrom] = useState(() => startOfDay(new Date()));
  const [events, setEvents] = useState<CalendarEvent[]>([]);
  const [reload, setReload] = useState(0);
  const [draft, setDraft] = useState<Draft | null>(null);
  const to = useMemo(() => addDays(from, SPAN_DAYS), [from]);

  useEffect(() => {
    let live = true;
    api
      .listEvents(from.toISOString(), to.toISOString())
      .then((e) => live && setEvents(e))
      .catch((err) => live && onError(BackendError.from(err).message));
    return () => {
      live = false;
    };
  }, [from, to, reload, onError]);

  // Group by each local day an event touches within the window.
  const days = useMemo(() => {
    const out: { day: Date; items: CalendarEvent[] }[] = [];
    for (let i = 0; i < SPAN_DAYS; i++) {
      const day = addDays(from, i);
      const next = addDays(day, 1);
      const items = events.filter((e) => {
        const s = new Date(e.startsAt);
        const en = e.endsAt ? new Date(e.endsAt) : e.allDay ? addDays(startOfDay(s), 1) : s;
        return s < next && (en > day || (en.getTime() === s.getTime() && s >= day));
      });
      if (items.length) out.push({ day, items });
    }
    return out;
  }, [events, from]);

  const remove = async (id: number) => {
    try {
      await api.deleteEvent(id);
      onError(null);
    } catch (err) {
      onError(BackendError.from(err).message);
    } finally {
      setReload((k) => k + 1);
    }
  };

  return (
    <div>
      <div className="mb-4 flex items-center gap-2">
        <button type="button" aria-label="Earlier" onClick={() => setFrom((f) => addDays(f, -SPAN_DAYS))} className="grid size-8 place-items-center rounded-full bg-surface-strong text-muted hover:bg-surface-hover hover:text-fg">
          <ChevronLeft className="size-4" />
        </button>
        <button type="button" onClick={() => setFrom(startOfDay(new Date()))} className="h-8 rounded-full bg-surface-strong px-3.5 text-xs font-medium text-muted hover:text-fg">
          Today
        </button>
        <button type="button" aria-label="Later" onClick={() => setFrom((f) => addDays(f, SPAN_DAYS))} className="grid size-8 place-items-center rounded-full bg-surface-strong text-muted hover:bg-surface-hover hover:text-fg">
          <ChevronRight className="size-4" />
        </button>
        <span className="text-sm text-muted">
          {from.toLocaleDateString([], { day: "numeric", month: "short" })} – {addDays(to, -1).toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" })}
        </span>
        {!draft && (
          <button type="button" onClick={() => setDraft(newDraft(dayDiff(from) <= 0 && dayDiff(to) > 0 ? new Date() : from))} className="ml-auto flex h-8 items-center gap-1 rounded-lg bg-accent px-3 text-xs font-semibold text-bg">
            <Plus className="size-3.5" /> Add event
          </button>
        )}
      </div>

      {draft && (
        <EventForm
          key={draft.id ?? "new"}
          draft={draft}
          onCancel={() => setDraft(null)}
          onSaved={() => {
            setDraft(null);
            setReload((k) => k + 1);
          }}
          onError={onError}
        />
      )}

      {days.length === 0 ? (
        <div className="rounded-[20px] bg-surface py-12 text-center text-sm text-faint">No events in these two weeks.</div>
      ) : (
        <div className="space-y-3">
          {days.map(({ day, items }) => (
            <Panel key={day.toISOString()} title={<span className={dayDiff(day) === 0 ? "text-accent" : ""}>{formatDay(day)}{dayDiff(day) <= 1 && dayDiff(day) >= 0 ? ` · ${day.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" })}` : ""}</span>}>
              <ul className="divide-y divide-line">
                {items.map((e) => (
                  <li key={e.id} className="group flex items-start gap-3 py-2">
                    <span className="w-24 shrink-0 pt-0.5 font-mono text-xs text-muted">{eventTime(e, day)}</span>
                    <div className="min-w-0 flex-1">
                      <div className="text-sm text-fg">{e.title}</div>
                      {e.location && (
                        <div className="flex items-center gap-1 text-[11px] text-faint">
                          <MapPin className="size-3" /> {e.location}
                        </div>
                      )}
                      {e.notes && <div className="mt-0.5 line-clamp-2 text-[11px] text-faint">{e.notes}</div>}
                    </div>
                    <div className="flex gap-0.5 opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100">
                      <button type="button" aria-label={`Edit ${e.title}`} onClick={() => setDraft(draftOf(e))} className="grid size-7 place-items-center rounded-md text-faint hover:text-fg">
                        <Pencil className="size-3.5" />
                      </button>
                      <button type="button" aria-label={`Delete ${e.title}`} onClick={() => void remove(e.id)} className="grid size-7 place-items-center rounded-md text-faint hover:text-danger">
                        <Trash2 className="size-3.5" />
                      </button>
                    </div>
                  </li>
                ))}
              </ul>
            </Panel>
          ))}
        </div>
      )}
      <p className="mt-4 text-[11px] text-faint">
        This calendar lives in IGRIS only. Syncing with Google Calendar or Outlook isn't available yet.
      </p>
    </div>
  );
}

