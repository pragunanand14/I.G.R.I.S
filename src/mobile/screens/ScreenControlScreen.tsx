import { useEffect, useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import { Dot, ErrorText, Group, Screen, Section } from "../ui";

/**
 * Settings → Screen control: lets IGRIS read your screen and, for tasks you
 * approve, tap and type for you. Only Android's own settings can switch it on.
 */
export function ScreenControlScreen() {
  const ready = useAppStore((s) => s.backend) === "ready";
  const [on, setOn] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!ready) return;
    const check = () =>
      document.visibilityState === "visible" &&
      api
        .phoneScreenAccess()
        .then(setOn)
        .catch(() => setOn(null));
    check();
    // Coming back from Android's settings: look again.
    document.addEventListener("visibilitychange", check);
    return () => document.removeEventListener("visibilitychange", check);
  }, [ready]);

  const open = () =>
    void api
      .phoneScreenSettings()
      .then(() => setError(null))
      .catch((err) => setError(BackendError.from(err).message));

  return (
    <Screen title="Screen control" subtitle="Let IGRIS read your screen and use your phone for you." back="/settings">
      <Group>
        <div className="m-row">
          <span className="flex flex-1 items-center gap-2 text-[15px]">
            <Dot tone={on ? "ok" : "warn"} /> {on === null ? "Checking…" : on ? "On" : "Off"}
          </span>
          <button type="button" className="m-chip" onClick={open}>
            {on ? "Turn off" : "Turn on"}
          </button>
        </div>
      </Group>
      {error && <ErrorText>{error}</ErrorText>}

      <Section title="What it does">
        <ul className="m-muted list-disc space-y-1.5 pl-6 text-[15px]">
          <li>“What&apos;s on my screen?” — IGRIS reads the text and buttons of the app in front.</li>
          <li>“Turn on dark mode”, “reply to Mom saying I&apos;ll be late” — IGRIS asks you first, then taps, types and swipes.</li>
          <li>A <b>Stop</b> pill stays at the top while it works. Tap it any time.</li>
          <li>Sending, paying, deleting and similar always ask you again. Password fields are never read or typed.</li>
          <li>What&apos;s on screen goes to your AI provider while IGRIS works, like a screenshot would.</li>
        </ul>
      </Section>

      {!on && (
        <Section title="How to turn it on">
          <ol className="m-muted list-decimal space-y-1.5 pl-6 text-[15px]">
            <li>
              Tap <b>Turn on</b> above → <b>Installed apps</b> → <b>IGRIS screen control</b> → switch it on.
            </li>
            <li>
              If Android says it&apos;s a <b>restricted setting</b> (apps installed from a file): open Settings → Apps → IGRIS → ⋮ (top right) →{" "}
              <b>Allow restricted settings</b>, then try again.
            </li>
          </ol>
        </Section>
      )}
    </Screen>
  );
}
