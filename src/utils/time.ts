/** Local-time formatting for tasks, reminders and events. */

const DAY_MS = 86_400_000;

export function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

export function addDays(d: Date, n: number): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
}

/** Whole local days from `now`'s day to `d`'s day (negative = past). */
export function dayDiff(d: Date, now = new Date()): number {
  return Math.round((startOfDay(d).getTime() - startOfDay(now).getTime()) / DAY_MS);
}

export function formatClock(d: Date): string {
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
}

/** "Today", "Tomorrow", "Yesterday", "Fri 9 Oct", or with a year when not this year. */
export function formatDay(d: Date, now = new Date()): string {
  const diff = dayDiff(d, now);
  if (diff === 0) return "Today";
  if (diff === 1) return "Tomorrow";
  if (diff === -1) return "Yesterday";
  const opts: Intl.DateTimeFormatOptions = { weekday: "short", day: "numeric", month: "short" };
  if (d.getFullYear() !== now.getFullYear()) opts.year = "numeric";
  return d.toLocaleDateString([], opts);
}

/** "Today 17:00", "Fri 9 Oct 09:30". */
export function formatWhen(iso: string, now = new Date()): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return `${formatDay(d, now)} ${formatClock(d)}`;
}

/** Countdown "4:05", "1:02:03"; "0:00" once elapsed. */
export function formatCountdown(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/** "in 5 min", "in 2 h 10 min", "in 3 days". */
export function formatRelative(ms: number): string {
  const mins = Math.round(ms / 60_000);
  if (Math.abs(ms) < 60_000) return ms >= 0 ? "in under a minute" : "just now";
  const abs = Math.abs(mins);
  const text = abs < 60 ? `${abs} min` : abs < 48 * 60 ? `${Math.floor(abs / 60)} h${abs % 60 ? ` ${abs % 60} min` : ""}` : `${Math.round(abs / 1440)} days`;
  return mins >= 0 ? `in ${text}` : `${text} ago`;
}

/** Local `YYYY-MM-DD` and `HH:MM` for <input type="date|time">. */
export function toDateInput(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function toTimeInput(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** Combine local date + time inputs into a UTC ISO string ("" if invalid). */
export function fromInputs(date: string, time = "00:00"): string {
  const [y, mo, d] = date.split("-").map(Number);
  const [h, mi] = time.split(":").map(Number);
  if (!y || !mo || !d || Number.isNaN(h) || Number.isNaN(mi)) return "";
  const t = new Date(y, mo - 1, d, h, mi);
  return Number.isNaN(t.getTime()) ? "" : t.toISOString();
}
