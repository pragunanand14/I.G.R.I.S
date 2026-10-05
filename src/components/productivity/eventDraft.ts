import type { CalendarEvent, EventInput } from "@/types/productivity";
import { fromInputs, toDateInput, toTimeInput } from "@/utils/time";

export interface Draft {
  id: number | null;
  title: string;
  date: string;
  start: string;
  end: string;
  allDay: boolean;
  location: string;
  notes: string;
}

export function newDraft(day: Date): Draft {
  return { id: null, title: "", date: toDateInput(day), start: "09:00", end: "10:00", allDay: false, location: "", notes: "" };
}

export function draftOf(e: CalendarEvent): Draft {
  const s = new Date(e.startsAt);
  const en = e.endsAt ? new Date(e.endsAt) : null;
  return { id: e.id, title: e.title, date: toDateInput(s), start: toTimeInput(s), end: en ? toTimeInput(en) : "", allDay: e.allDay, location: e.location, notes: e.notes };
}

/** Draft → EventInput. Same-day end times; an end before the start rolls into the next day. */
export function draftToInput(d: Draft): EventInput | string {
  const startsAt = fromInputs(d.date, d.allDay ? "00:00" : d.start);
  if (!startsAt) return "Pick a valid date and start time.";
  let endsAt = "";
  if (!d.allDay && d.end) {
    endsAt = fromInputs(d.date, d.end);
    if (!endsAt) return "The end time isn't valid.";
    if (new Date(endsAt) <= new Date(startsAt)) {
      const next = new Date(endsAt);
      next.setDate(next.getDate() + 1);
      endsAt = next.toISOString();
    }
  }
  return { title: d.title, startsAt, endsAt, allDay: d.allDay, location: d.location, notes: d.notes };
}

