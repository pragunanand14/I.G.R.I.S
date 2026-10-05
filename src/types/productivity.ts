export type Priority = "low" | "normal" | "high";

export interface Task {
  id: number;
  title: string;
  notes: string;
  /** UTC RFC 3339, or null. */
  dueAt: string | null;
  priority: Priority;
  done: boolean;
  completedAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface TaskInput {
  title: string;
  notes: string;
  /** UTC RFC 3339 or "" for none. */
  dueAt: string;
  priority: Priority;
}

export type ReminderStatus = "pending" | "fired" | "dismissed" | "cancelled";

export interface Reminder {
  id: number;
  title: string;
  kind: "reminder" | "timer";
  dueAt: string;
  durationSecs: number | null;
  status: ReminderStatus;
  firedAt: string | null;
  createdAt: string;
}

export interface FiredReminder extends Reminder {
  /** Went off late because IGRIS wasn't running at the due time. */
  late: boolean;
}

export interface CalendarEvent {
  id: number;
  title: string;
  startsAt: string;
  endsAt: string | null;
  allDay: boolean;
  location: string;
  notes: string;
  createdAt: string;
  updatedAt: string;
}

export interface EventInput {
  title: string;
  startsAt: string;
  endsAt: string;
  allDay: boolean;
  location: string;
  notes: string;
}

export interface ParsedTime {
  at: string;
  description: string;
}

export function toTaskInput(t: Task): TaskInput {
  return { title: t.title, notes: t.notes, dueAt: t.dueAt ?? "", priority: t.priority };
}

export function toEventInput(e: CalendarEvent): EventInput {
  return { title: e.title, startsAt: e.startsAt, endsAt: e.endsAt ?? "", allDay: e.allDay, location: e.location, notes: e.notes };
}
