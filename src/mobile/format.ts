/** "in 5 min", "at 17:30", "tomorrow at 09:00", "Mon 6 Oct at 14:00". */
export function whenText(iso: string, now: Date = new Date()): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const mins = Math.round((d.getTime() - now.getTime()) / 60000);
  const time = d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  if (mins >= 0 && mins < 60) return mins <= 1 ? "in a minute" : `in ${mins} min`;
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((day(d) - day(now)) / 86400000);
  if (days === 0) return `today at ${time}`;
  if (days === 1) return `tomorrow at ${time}`;
  if (days === -1) return `yesterday at ${time}`;
  return `${d.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" })} at ${time}`;
}
