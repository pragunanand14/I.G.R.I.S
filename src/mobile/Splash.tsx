import { useEffect, useRef, useState } from "react";
import { AiCore } from "@/components/core/AiCore";
import { useAppStore } from "@/stores/appStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { reducedMotion, useIntro } from "./motion";

const TEXT = "I.G.R.I.S is starting…";
const TYPE_MS = 42;
/** Shown at least this long, so the start doesn't flicker. */
const MIN_MS = 1300;
/** …and at most this long while waiting for the engine (it then carries on; Home says what's wrong). */
const MAX_WAIT_MS = 4000;
const EASE = "cubic-bezier(0.65, 0, 0.25, 1)";

/**
 * The opening: the IGRIS orb with "I.G.R.I.S is starting…" typed under it,
 * then the orb glides and shrinks into its place on Home while Home fades in.
 * Elsewhere (or with reduced motion) it simply fades.
 */
export function Splash() {
  const state = useIntro((s) => s.state);
  const finish = useIntro((s) => s.finish);
  const backend = useAppStore((s) => s.backend);
  const settings = useSettingsStore((s) => s.status);
  const [typed, setTyped] = useState(0);
  const [elapsed, setElapsed] = useState(false);
  const [timedOut, setTimedOut] = useState(false);
  const [gone, setGone] = useState(false);
  const leaving = useRef(false);
  const root = useRef<HTMLDivElement>(null);
  const bg = useRef<HTMLDivElement>(null);
  const orb = useRef<HTMLDivElement>(null);
  const text = useRef<HTMLParagraphElement>(null);

  useEffect(() => {
    const t = setInterval(() => setTyped((n) => (n >= TEXT.length ? n : n + 1)), TYPE_MS);
    const a = setTimeout(() => setElapsed(true), MIN_MS);
    const b = setTimeout(() => setTimedOut(true), MAX_WAIT_MS);
    return () => {
      clearInterval(t);
      clearTimeout(a);
      clearTimeout(b);
    };
  }, []);

  // Ready to go once the engine answered (and, if it's running, the settings — so the theme doesn't change mid-way).
  const engineSettled = backend !== "connecting" && (backend !== "ready" || settings === "ready" || settings === "error");
  const canLeave = state === "playing" && typed >= TEXT.length && elapsed && (engineSettled || timedOut);

  useEffect(() => {
    if (!canLeave || leaving.current) return;
    leaving.current = true;
    const target = document.querySelector<HTMLElement>("[data-orb='home']");
    const done = () => {
      finish();
      // The orb on Home fades in as this one fades out, then the splash is removed.
      const out = root.current?.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 220, easing: "ease-out", fill: "forwards" });
      if (out) out.onfinish = () => setGone(true);
      else setGone(true);
    };
    if (!target || !orb.current || !bg.current || reducedMotion() || typeof orb.current.animate !== "function") {
      const fade = root.current?.animate?.([{ opacity: 1 }, { opacity: 0 }], { duration: 300, easing: "ease-out", fill: "forwards" });
      finish();
      if (fade) fade.onfinish = () => setGone(true);
      else setGone(true);
      return;
    }
    const from = orb.current.getBoundingClientRect();
    const to = target.getBoundingClientRect();
    const dx = to.left + to.width / 2 - (from.left + from.width / 2);
    const dy = to.top + to.height / 2 - (from.top + from.height / 2);
    const scale = to.width / from.width;
    text.current?.animate(
      [
        { opacity: 1, transform: "translateY(0)" },
        { opacity: 0, transform: "translateY(8px)" },
      ],
      { duration: 260, easing: "ease-in", fill: "forwards" },
    );
    bg.current.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 650, delay: 200, easing: "ease-in-out", fill: "forwards" });
    const move = orb.current.animate([{ transform: "translate(0, 0) scale(1)" }, { transform: `translate(${dx}px, ${dy}px) scale(${scale})` }], {
      duration: 850,
      delay: 120,
      easing: EASE,
      fill: "forwards",
    });
    move.onfinish = done;
  }, [canLeave, finish]);

  if (gone) return null;
  return (
    <div ref={root} className="m-splash" aria-live="polite">
      <div ref={bg} className="m-splash-bg" />
      <div className="m-splash-center">
        <div ref={orb} className="m-splash-orb">
          <AiCore state={backend === "unavailable" || backend === "error" ? "error" : "thinking"} size="100%" />
        </div>
        <p ref={text} className="m-splash-text" role="status">
          {TEXT.slice(0, typed)}
          <span className="m-caret" aria-hidden="true" />
        </p>
      </div>
    </div>
  );
}
