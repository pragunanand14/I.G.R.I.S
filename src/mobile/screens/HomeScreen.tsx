import { ArrowUp, CalendarCheck, MessagesSquare, Mic, Settings } from "lucide-react";
import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import { Link } from "react-router";
import { AiCore } from "@/components/core/AiCore";
import { useCoreState } from "@/hooks/useCoreState";
import { api } from "@/services/api";
import { useAppStore } from "@/stores/appStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useVoiceStore } from "@/stores/voiceStore";
import { greetingFor } from "@/utils/format";
import { useAsk } from "../ask";
import { nav, useIntro } from "../motion";
import { useStatus } from "../status";
import { Dot } from "../ui";

/** The orb, a greeting, one bar to type or talk, and a way to everything else. */
export function HomeScreen() {
  const intro = useIntro((s) => s.state);
  // Animate Home in only when it appears at the end of the opening animation.
  const [entering] = useState(intro === "playing");
  const enter = (i: number, base: string) =>
    entering
      ? { className: `${base} ${intro === "playing" ? "m-intro-wait" : "m-intro-in"}`, style: { animationDelay: `${i * 70}ms` } }
      : { className: base };
  const core = useCoreState();
  const userName = useSettingsStore((s) => s.settings.userName).trim();

  return (
    <div className="m-home">
      <header {...enter(0, "flex h-14 shrink-0 items-center justify-between")}>
        <IconLink to="/chats" label="Your chats">
          <MessagesSquare className="size-[22px]" />
        </IconLink>
        <div className="flex items-center gap-1">
          <TodayLink />
          <IconLink to="/settings" label="Settings">
            <Settings className="size-[22px]" />
          </IconLink>
        </div>
      </header>

      <div className="flex min-h-0 flex-1 flex-col items-center justify-center text-center">
        <div data-orb="home" className="m-home-orb" style={{ opacity: intro === "playing" ? 0 : 1 }}>
          <AiCore state={core} size="100%" />
        </div>
        <h1 {...enter(1, "mt-7 text-[26px] font-semibold tracking-tight")}>
          {greetingFor(new Date())}
          {userName ? `, ${userName}` : ""}
        </h1>
        <div {...enter(2, "mt-2 w-full")}>
          <StatusLine />
        </div>
      </div>

      <div {...enter(3, "shrink-0 pt-4 pb-[calc(16px+env(safe-area-inset-bottom))]")}>
        <AskBar />
      </div>
    </div>
  );
}

function IconLink({ to, label, children, dot }: { to: string; label: string; children: ReactNode; dot?: boolean }) {
  return (
    <Link to={to} viewTransition={nav().viewTransition} className="m-icon-button relative" aria-label={label} title={label}>
      {children}
      {dot && <span className="absolute top-2.5 right-2.5 size-2 rounded-full" style={{ background: "var(--m-accent)" }} aria-hidden="true" />}
    </Link>
  );
}

/** Reminders and to-dos; a dot when something is due within a day or open. */
function TodayLink() {
  const backend = useAppStore((s) => s.backend);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (backend !== "ready") return;
    let live = true;
    const load = () =>
      Promise.all([api.listReminders(), api.listTasks("open")])
        .then(([r, t]) => {
          const soon = Date.now() + 86400000;
          if (live) setBusy(t.length > 0 || r.some((x) => x.status === "pending" && new Date(x.dueAt).getTime() < soon));
        })
        .catch(() => undefined);
    void load();
    const id = setInterval(() => void load(), 60000);
    return () => {
      live = false;
      clearInterval(id);
    };
  }, [backend]);
  return (
    <IconLink to="/today" label="Reminders and to-dos" dot={busy}>
      <CalendarCheck className="size-[22px]" />
    </IconLink>
  );
}

/** What IGRIS is doing, in a few words; the reason and a way to fix it when it can't help. */
function StatusLine() {
  const status = useStatus();
  const phase = useVoiceStore((s) => s.phase);
  if (!status.ready)
    return (
      <div className="m-card m-pop mx-auto mt-3 max-w-sm px-4 py-3 text-left" role="status">
        <p className="flex items-center gap-2 font-medium">
          <Dot tone={status.tone} /> {status.title}
        </p>
        <p className="m-muted mt-1 text-sm break-words">{status.detail}</p>
        {status.needsSetup && (
          <Link to="/settings/ai" viewTransition={nav().viewTransition} className="m-link mt-2 inline-flex min-h-9 items-center text-[15px]">
            How to set it up
          </Link>
        )}
      </div>
    );
  const text = phase === "listening" ? "Listening… tap the mic when you're done" : phase === "transcribing" ? "Writing down what you said…" : status.title;
  return (
    <p key={text} className="m-muted m-fade-in text-[15px]" role="status">
      {text}
    </p>
  );
}

/** Type, or tap the mic and talk. While there's text, the mic becomes send. */
function AskBar() {
  const status = useStatus();
  const ask = useAsk();
  const [text, setText] = useState("");
  const phase = useVoiceStore((s) => s.phase);
  useVoiceStore((s) => s.status);
  const can = useVoiceStore.getState().canListen();
  const toggle = useVoiceStore((s) => s.toggle);
  const listening = phase === "listening";
  const typing = text.trim().length > 0;

  const submit = (e?: FormEvent) => {
    e?.preventDefault();
    const t = text.trim();
    if (!t || !status.ready) return;
    setText("");
    void ask(t);
  };
  const micLabel = listening ? "Stop listening" : phase === "transcribing" ? "Writing down what you said…" : can.ok ? "Talk to IGRIS" : (can.reason ?? "Voice isn't available");
  const disabled = typing ? !status.ready : (!status.ready || !can.ok || phase === "transcribing") && !listening;

  return (
    <form onSubmit={submit} className="m-ask">
      <input
        className="m-field min-h-0 flex-1"
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder={status.ready ? "Ask anything" : "IGRIS isn't ready yet"}
        aria-label="Ask IGRIS"
        disabled={!status.ready}
        enterKeyHint="send"
      />
      <button
        type={typing ? "submit" : "button"}
        className="m-mic"
        data-mode={typing ? "send" : "mic"}
        aria-label={typing ? "Send" : micLabel}
        title={typing ? "Send" : micLabel}
        aria-pressed={typing ? undefined : listening}
        disabled={disabled}
        onClick={typing ? undefined : () => void toggle()}
      >
        <Mic className="m-mic-icon m-mic-icon--mic size-6" />
        <ArrowUp className="m-mic-icon m-mic-icon--send size-6" />
      </button>
    </form>
  );
}
