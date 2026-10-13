import { useEffect, useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import { Dot, ErrorText, Group, Screen, Section, Switch } from "../ui";
import { RestrictedHelp } from "../RestrictedHelp";

/**
 * Settings → Screen control: lets IGRIS read your screen and, for tasks you
 * approve, tap and type for you. Only Android's own settings can switch it on.
 */
export function ScreenControlScreen() {
  const ready = useAppStore((s) => s.backend) === "ready";
  const [on, setOn] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [askRisky, setAskRisky] = useState(false);

  useEffect(() => {
    if (!ready) return;
    api
      .phoneScreenAskRisky()
      .then(setAskRisky)
      .catch(() => undefined);
  }, [ready]);

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
    <Screen
      title="Screen control"
      subtitle="Let IGRIS read your screen and use your phone for you."
      back="/settings"
    >
      <Group>
        <div className="m-row">
          <span className="flex flex-1 items-center gap-2 text-[15px]">
            <Dot tone={on ? "ok" : "warn"} />{" "}
            {on === null ? "Checking…" : on ? "On" : "Off"}
          </span>
          <button type="button" className="m-chip" onClick={open}>
            {on ? "Turn off" : "Turn on"}
          </button>
        </div>
      </Group>
      {error && <ErrorText>{error}</ErrorText>}

      <Group>
        <Switch
          label="Ask before sending or deleting"
          description="Off: once you approve a task, IGRIS does it all the way through. On: it also checks with you before it sends, deletes or posts something."
          checked={askRisky}
          disabled={!ready}
          onChange={(v) =>
            api
              .phoneSetScreenAskRisky(v)
              .then(setAskRisky)
              .catch((err) => setError(BackendError.from(err).message))
          }
        />
      </Group>

      <Section title="What it does">
        <ul className="m-muted list-disc space-y-1.5 pl-6 text-[15px]">
          <li>
            “What&apos;s on my screen?” — IGRIS reads the text and buttons of
            the app in front.
          </li>
          <li>
            “Find Amul milk on Blinkit and add it to the cart”, “turn on dark
            mode” — you approve the task once, then IGRIS taps, types, scrolls,
            allows what the app asks for, turns on location and retries until
            it&apos;s done.
          </li>
          <li>
            A <b>Stop</b> pill stays at the top while it works. Tap it any time.
          </li>
          <li>
            <b>IGRIS never pays.</b> It stops at the payment step and leaves it
            to you; payment and banking apps are off limits. Passwords, PINs,
            card details and one-time codes are never read or typed.
          </li>
          <li>
            What&apos;s on screen goes to your AI provider while IGRIS works,
            like a screenshot would.
          </li>
        </ul>
      </Section>

      {!on && (
        <Section title="How to turn it on">
          <ol className="m-muted list-decimal space-y-1.5 pl-6 text-[15px]">
            <li>
              Tap <b>Turn on</b> above → <b>Installed apps</b> →{" "}
              <b>IGRIS screen control</b> → switch it on.
            </li>
            <RestrictedHelp setting="IGRIS screen control" />
          </ol>
        </Section>
      )}
    </Screen>
  );
}
