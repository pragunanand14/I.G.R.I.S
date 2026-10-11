import { Sunrise } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import type { Briefing } from "@/types/briefing";

const PARTS: { key: "weather" | "calendar" | "email" | "tasks"; label: string }[] = [
  { key: "weather", label: "Weather" },
  { key: "calendar", label: "Meetings" },
  { key: "email", label: "Important emails" },
  { key: "tasks", label: "To-dos & reminders" },
];

/** Set up the daily briefing: when, what's in it, news topics, home city. */
export function BriefingSettings({ phone = false, onSaved }: { phone?: boolean; onSaved?: () => void }) {
  const [b, setB] = useState<Briefing | null>(null);
  const [topics, setTopics] = useState("");
  const [place, setPlace] = useState<string | null>(null);
  const [city, setCity] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    api
      .briefingGet()
      .then((v) => {
        setB(v);
        setTopics(v.news.join(", "));
      })
      .catch((err) => setNote({ ok: false, text: BackendError.from(err).message }));
    api
      .weatherHome()
      .then((p) => setPlace(p?.name ?? null))
      .catch(() => undefined);
  }, []);

  if (!b) return note ? <p className="text-sm text-danger">{note.text}</p> : null;

  const save = async (next: Briefing) => {
    setBusy(true);
    setNote(null);
    try {
      if (city.trim()) {
        const p = await api.weatherSetHome(city);
        setPlace(p.name);
        setCity("");
      }
      const news = topics
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean);
      const saved = await api.briefingSet({ ...next, news });
      setB(saved);
      setNote({ ok: true, text: saved.enabled ? `Your briefing is set for ${saved.weekdaysOnly ? "weekdays" : "every day"} at ${saved.time}.` : "Briefing turned off." });
      onSaved?.();
    } catch (err) {
      setNote({ ok: false, text: BackendError.from(err).message });
    } finally {
      setBusy(false);
    }
  };

  const label = phone ? "text-[15px]" : "text-sm text-fg";
  const muted = phone ? "m-muted text-sm" : "text-xs text-muted";
  const input = phone ? "m-input" : "w-full rounded-xl bg-surface-strong px-3 py-2 text-sm text-fg placeholder:text-faint focus:outline-none";

  return (
    <div className={phone ? "m-card space-y-3 px-4 py-4" : "space-y-3 rounded-2xl bg-surface p-4"}>
      <div className="flex items-center gap-3">
        <Sunrise className="size-5 shrink-0 text-accent" aria-hidden="true" />
        <div className="min-w-0 flex-1">
          <p className={phone ? "text-[16px] font-semibold" : "text-sm font-semibold text-fg"}>Daily briefing</p>
          <p className={muted}>A short morning summary — as a notification, and on Home with Listen.</p>
        </div>
        <input
          type="checkbox"
          role="switch"
          aria-label="Daily briefing on"
          checked={b.enabled}
          disabled={busy}
          onChange={(e) => void save({ ...b, enabled: e.target.checked })}
          className="size-5 shrink-0"
        />
      </div>

      <div className="flex flex-wrap items-center gap-3">
        <label className={`flex items-center gap-2 ${label}`}>
          At
          <input type="time" aria-label="Briefing time" value={b.time} onChange={(e) => setB({ ...b, time: e.target.value })} className={`${input} w-auto`} />
        </label>
        <label className={`flex items-center gap-2 ${label}`}>
          <input type="checkbox" checked={b.weekdaysOnly} onChange={(e) => setB({ ...b, weekdaysOnly: e.target.checked })} />
          Weekdays only
        </label>
      </div>

      <fieldset className="flex flex-wrap gap-x-4 gap-y-2">
        <legend className={`mb-1.5 ${muted}`}>Include</legend>
        {PARTS.map((p) => (
          <label key={p.key} className={`flex items-center gap-2 ${label}`}>
            <input type="checkbox" checked={b[p.key]} onChange={(e) => setB({ ...b, [p.key]: e.target.checked })} />
            {p.label}
          </label>
        ))}
      </fieldset>

      <label className="block space-y-1">
        <span className={muted}>News about (optional, comma-separated — needs web search)</span>
        <input className={input} value={topics} maxLength={300} placeholder="e.g. cricket, AI, Mumbai" onChange={(e) => setTopics(e.target.value)} />
      </label>

      <label className="block space-y-1">
        <span className={muted}>{place ? `Weather for ${place} — change:` : "Your city, for the weather"}</span>
        <input className={input} value={city} maxLength={80} placeholder={place ? "Another city" : "e.g. Mumbai"} onChange={(e) => setCity(e.target.value)} />
      </label>

      <button
        type="button"
        disabled={busy}
        onClick={() => void save({ ...b, enabled: true })}
        className={phone ? "m-button m-button-primary w-full" : "rounded-full bg-accent px-4 py-2 text-sm font-semibold text-bg hover:opacity-90 disabled:opacity-40"}
      >
        {busy ? "Saving…" : b.enabled ? "Save" : "Turn on the briefing"}
      </button>
      {note && <p className={`${muted} ${note.ok ? "" : "text-danger"}`}>{note.text}</p>}
    </div>
  );
}
