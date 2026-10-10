import { CalendarClock, CloudSun, Sparkles } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import { ErrorText, Group, Screen, Section } from "../ui";

const WIDGETS = [
  { icon: Sparkles, name: "IGRIS orb", what: "Tap it and IGRIS opens, already listening." },
  { icon: CalendarClock, name: "IGRIS clock", what: "The time and date, in IGRIS's style." },
  { icon: CloudSun, name: "IGRIS weather", what: "Now, and today's high and low, for the place below." },
];

/** Settings → Home-screen widgets: what's there, how to add them, the weather place. */
export function WidgetsScreen() {
  const ready = useAppStore((s) => s.backend) === "ready";
  const [place, setPlace] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!ready) return;
    api
      .widgetWeatherPlace()
      .then(setPlace)
      .catch(() => undefined);
  }, [ready]);

  const save = () => {
    setBusy(true);
    setError(null);
    api
      .widgetSetWeatherPlace(query)
      .then((p) => {
        setPlace(p.name);
        setQuery("");
      })
      .catch((err) => setError(BackendError.from(err).message))
      .finally(() => setBusy(false));
  };

  return (
    <Screen title="Home-screen widgets" subtitle="Touch and hold an empty spot on your home screen → Widgets → IGRIS." back="/settings">
      <Group>
        {WIDGETS.map(({ icon: Icon, name, what }) => (
          <div key={name} className="m-row">
            <Icon className="size-5 shrink-0" style={{ color: "var(--m-accent)" }} aria-hidden="true" />
            <div className="min-w-0 flex-1">
              <p className="text-[15px] font-medium">{name}</p>
              <p className="m-muted text-sm">{what}</p>
            </div>
          </div>
        ))}
      </Group>

      <Section title="Weather place">
        <p className="m-muted px-1 text-[15px]">
          {place ? (
            <>
              Showing the weather for <b>{place}</b>.
            </>
          ) : (
            "Choose a city or town for the weather widget. Only that place is sent to the weather service (Open-Meteo) — never your location."
          )}
        </p>
        {error && <ErrorText>{error}</ErrorText>}
        <input
          aria-label="City or town"
          className="m-input"
          placeholder="e.g. Mumbai"
          value={query}
          maxLength={80}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && query.trim() && !busy && save()}
        />
        <button type="button" className="m-button m-button-primary w-full" disabled={!ready || busy || !query.trim()} onClick={save}>
          {busy ? "Looking it up…" : place ? "Change place" : "Use this place"}
        </button>
      </Section>
    </Screen>
  );
}
