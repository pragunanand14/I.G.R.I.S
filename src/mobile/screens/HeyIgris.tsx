import { useCallback, useEffect, useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import type { WakeStatus } from "@/types/voice";
import { Dot, Group, Row, Section, Switch } from "../ui";

/**
 * "Hey IGRIS" / "Wake up IGRIS" (heard on the phone itself) and IGRIS as the
 * phone's digital assistant (the assistant gesture opens it listening).
 */
export function HeyIgris({ ready }: { ready: boolean }) {
  const [s, setS] = useState<WakeStatus | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [tile, setTile] = useState<string | null>(null);

  const load = useCallback(() => {
    api
      .phoneWakeStatus()
      .then(setS, (err) => setNote(BackendError.from(err).message));
  }, []);

  // Coming back from Android's settings pages: show what changed.
  useEffect(() => {
    if (!ready) return;
    load();
    const onShow = () => document.visibilityState === "visible" && load();
    document.addEventListener("visibilitychange", onShow);
    return () => document.removeEventListener("visibilitychange", onShow);
  }, [ready, load]);

  const set = (on: boolean, chargingOnly: boolean) => {
    setNote(null);
    api.phoneWakeSet(on, chargingOnly).then(
      () => setTimeout(load, 800),
      (err) => {
        setNote(BackendError.from(err).message);
        load();
      },
    );
  };
  const open = (page: "overlay" | "battery" | "assistant") =>
    void api
      .phoneOpenSettings(page)
      .catch((err) => setNote(BackendError.from(err).message));

  if (!s) return note ? <p className="m-muted px-1 text-sm">{note}</p> : null;
  const state =
    s.problem ??
    (!s.on
      ? "Off"
      : s.listening
        ? "Listening for its name"
        : s.running
          ? "Paused (charger, or IGRIS is using the mic)"
          : "Starting…");

  return (
    <>
      <Section title="Hey IGRIS">
        <Group>
          <Switch
            label="Listen for “Hey IGRIS” / “Wake up IGRIS”"
            description="Works with IGRIS closed and the screen off. The name is recognised on this phone; nothing is sent anywhere until IGRIS hears it. Music keeps playing normally."
            checked={s.on}
            onChange={(on) => set(on, s.chargingOnly)}
          />
          {s.on && (
            <>
              <Switch
                label="Only while charging"
                description="Saves battery."
                checked={s.chargingOnly}
                onChange={(c) => set(true, c)}
              />
              <Row
                title={
                  <span className="flex items-center gap-2">
                    <Dot
                      tone={s.problem ? "bad" : s.listening ? "ok" : "warn"}
                    />
                    {state}
                  </span>
                }
              />
              <Row
                title="Let IGRIS open itself"
                detail={
                  s.canOpenItself
                    ? "Allowed — IGRIS pops up when it hears its name."
                    : "Needed so IGRIS can pop up from the background: turn on “Display over other apps”."
                }
                value={s.canOpenItself ? "On" : "Turn on"}
                onClick={s.canOpenItself ? undefined : () => open("overlay")}
              />
              <Row
                title="Don't let battery saving stop it"
                detail={
                  s.batteryUnrestricted
                    ? "IGRIS won't be stopped in the background."
                    : "Samsung stops background apps after a while. Allow IGRIS to run unrestricted."
                }
                value={s.batteryUnrestricted ? "On" : "Allow"}
                onClick={
                  s.batteryUnrestricted ? undefined : () => open("battery")
                }
              />
            </>
          )}
        </Group>
        <p className="m-faint px-1 text-sm">
          Phones keep the low-power “Hey Google” chip for Google and Samsung’s
          own assistants, so IGRIS uses the normal microphone while this is on
          (Android shows a small “Hey IGRIS is on” notification). After the
          phone restarts, open IGRIS once to turn it back on. “Wake up IGRIS” is
          recognised most reliably.
        </p>
      </Section>

      <Section title="Phone assistant">
        <Group>
          <Row
            title={
              <span className="flex items-center gap-2">
                <Dot tone={s.assistant ? "ok" : "warn"} />
                {s.assistant
                  ? "IGRIS is your phone's assistant"
                  : "Make IGRIS your phone's assistant"}
              </span>
            }
            detail={
              s.assistant
                ? "Swipe up from a bottom corner (or hold the Home button) to talk to IGRIS."
                : "Opens Android's settings: Digital assistant app → Device assistance app → IGRIS."
            }
            value={s.assistant ? undefined : "Set"}
            onClick={() => open("assistant")}
          />
          <Row
            title="IGRIS in Quick Settings"
            detail={
              tile ??
              "A tile in the swipe-down panel: one tap opens IGRIS listening, from any app."
            }
            value="Add"
            onClick={() =>
              api
                .phoneAddQuickTile()
                .then((r) =>
                  setTile(
                    r === "added" || r === "already"
                      ? "Added — swipe down from the top to find it."
                      : r === "not_added"
                        ? "Not added. Tap Add again whenever you like."
                        : "Swipe down twice → ✎ (edit) → drag the IGRIS tile into your tiles.",
                  ),
                )
                .catch((err) => setNote(BackendError.from(err).message))
            }
          />
        </Group>
        <p className="m-faint px-1 text-sm">
          On Samsung: Settings → Apps → Choose default apps → Digital assistant
          app → Device assistance app → IGRIS. Then swipe up from a bottom
          corner of the screen (gesture navigation), or press and hold the Home
          button (navigation buttons), and IGRIS opens listening. “Hey Google”
          keeps working for Google’s assistant.
        </p>
      </Section>
      {note && <p className="m-muted px-1 text-sm">{note}</p>}
    </>
  );
}
