import { useEffect, useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import { Dot, ErrorText, Group, Screen, Section, Switch } from "../ui";

/**
 * Settings → Messages: let IGRIS read message notifications (summaries,
 * replies). Only Android's own settings can switch it on.
 */
export function MessagesScreen() {
  const ready = useAppStore((s) => s.backend) === "ready";
  const [on, setOn] = useState<boolean | null>(null);
  const [allApps, setAllApps] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!ready) return;
    const check = () =>
      document.visibilityState === "visible" &&
      api
        .phoneMessagesAccess()
        .then(setOn)
        .catch(() => setOn(null));
    check();
    api
      .phoneMessagesAllApps()
      .then(setAllApps)
      .catch(() => undefined);
    document.addEventListener("visibilitychange", check);
    return () => document.removeEventListener("visibilitychange", check);
  }, [ready]);

  const open = () =>
    void api
      .phoneMessagesSettings()
      .then(() => setError(null))
      .catch((err) => setError(BackendError.from(err).message));

  return (
    <Screen title="Messages" subtitle="Let IGRIS catch you up on messages and reply for you." back="/settings">
      <Group>
        <div className="m-row">
          <span className="flex flex-1 items-center gap-2 text-[15px]">
            <Dot tone={on ? "ok" : "warn"} /> {on === null ? "Checking…" : on ? "On" : "Off"}
          </span>
          <button type="button" className="m-chip" onClick={open}>
            {on ? "Turn off" : "Turn on"}
          </button>
        </div>
        <Switch
          label="All apps' notifications"
          description="Off: only messaging apps (WhatsApp, SMS, Telegram, Gmail, Instagram, …)."
          checked={allApps}
          disabled={!ready}
          onChange={(v) =>
            void api
              .phoneSetMessagesAllApps(v)
              .then(setAllApps)
              .catch((err) => setError(BackendError.from(err).message))
          }
        />
      </Group>
      {error && <ErrorText>{error}</ErrorText>}

      <Section title="What it does">
        <ul className="m-muted list-disc space-y-1.5 pl-6 text-[15px]">
          <li>“Any messages?” — a quick summary: who wrote, and what they need.</li>
          <li>“Reply to Mom that I’ll be there at 8” — through WhatsApp/SMS’s own reply. You confirm every reply.</li>
          <li>“Text Dad I’m running late” — opens the chat with the message typed in. “Call Dad” — asks first, then calls.</li>
          <li>One-time codes (OTPs) are hidden before IGRIS sees them. Messages are kept on the phone for a day, in memory only.</li>
          <li>The messages IGRIS reads go to your AI provider so it can summarise them.</li>
        </ul>
      </Section>

      {!on && (
        <Section title="How to turn it on">
          <ol className="m-muted list-decimal space-y-1.5 pl-6 text-[15px]">
            <li>
              Tap <b>Turn on</b> above → find <b>IGRIS messages</b> → allow.
            </li>
            <li>
              If Android says it&apos;s a <b>restricted setting</b>: Settings → Apps → IGRIS → ⋮ (top right) → <b>Allow restricted settings</b>, then try
              again.
            </li>
            <li>The first time you ask IGRIS to text or call someone by name, Android asks to let it see your contacts / make calls.</li>
          </ol>
        </Section>
      )}
    </Screen>
  );
}
