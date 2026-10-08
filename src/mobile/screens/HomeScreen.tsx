import { ArrowUp, Bell, Mic } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";
import { SUGGESTIONS, useAsk } from "../ask";
import { api } from "@/services/api";
import { useAppStore } from "@/stores/appStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useVoiceStore } from "@/stores/voiceStore";
import type { Reminder, Task } from "@/types/productivity";
import { greetingFor } from "@/utils/format";
import { useStatus } from "../status";
import { whenText } from "../format";
import { Dot, Group, Row, Section } from "../ui";

/** A greeting, one place to ask, and what's coming up. */
export function HomeScreen() {
  const status = useStatus();
  const userName = useSettingsStore((s) => s.settings.userName).trim();
  const ask = useAsk();
  const [text, setText] = useState("");
  const now = new Date();

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const t = text.trim();
    if (!t || !status.ready) return;
    setText("");
    void ask(t);
  };

  return (
    <div className="mx-auto flex min-h-full w-full max-w-xl flex-col px-5 pt-10 pb-8">
      <header>
        <p className="m-faint text-sm">{now.toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" })}</p>
        <h1 className="m-title mt-1.5">
          {greetingFor(now)}
          {userName ? `, ${userName}` : ""}
        </h1>
        <StatusLine />
      </header>

      <div className="mt-8 space-y-4">
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
          {text.trim() ? (
            <button type="submit" className="m-mic" aria-label="Send" disabled={!status.ready}>
              <ArrowUp className="size-6" />
            </button>
          ) : (
            <TalkButton disabled={!status.ready} />
          )}
        </form>

        <div className="m-scroll-x" aria-label="Try asking">
          {SUGGESTIONS.map((s) => (
            <button key={s} type="button" className="m-chip" disabled={!status.ready} onClick={() => void ask(s)}>
              {s}
            </button>
          ))}
        </div>
      </div>

      <div className="mt-10">
        <ComingUp />
      </div>
    </div>
  );
}

/** One quiet line when all is well; the reason (in the backend's words) and a way to fix it when not. */
function StatusLine() {
  const status = useStatus();
  if (status.ready)
    return (
      <p className="m-muted mt-3 flex items-center gap-2 text-sm" role="status">
        <Dot tone={status.tone} /> {status.title}
      </p>
    );
  return (
    <div className="m-card mt-5 p-4" role="status">
      <p className="flex items-center gap-2 font-medium">
        <Dot tone={status.tone} /> {status.title}
      </p>
      <p className="m-muted mt-1 text-sm break-words">{status.detail}</p>
      {status.needsSetup && (
        <Link to="/more/ai" className="m-link mt-3 inline-flex min-h-9 items-center text-[15px]">
          How to set it up
        </Link>
      )}
    </div>
  );
}

/** Tap to talk; tap again to stop. The words go to the chat. */
function TalkButton({ disabled }: { disabled: boolean }) {
  const phase = useVoiceStore((s) => s.phase);
  useVoiceStore((s) => s.status);
  const can = useVoiceStore.getState().canListen();
  const toggle = useVoiceStore((s) => s.toggle);
  const navigate = useNavigate();
  const listening = phase === "listening";
  const label = listening ? "Stop listening" : phase === "transcribing" ? "Writing down what you said…" : can.ok ? "Talk to IGRIS" : (can.reason ?? "Voice isn't available");
  return (
    <button
      type="button"
      className="m-mic"
      aria-label={label}
      title={label}
      aria-pressed={listening}
      disabled={(disabled || !can.ok || phase === "transcribing") && !listening}
      onClick={() => {
        void toggle();
        if (!listening) void navigate("/chat");
      }}
    >
      <Mic className="size-6" />
    </button>
  );
}

/** The next reminders and how many to-dos are open. */
function ComingUp() {
  const backend = useAppStore((s) => s.backend);
  const [reminders, setReminders] = useState<Reminder[] | null>(null);
  const [tasks, setTasks] = useState<Task[] | null>(null);
  useEffect(() => {
    if (backend !== "ready") return;
    let live = true;
    const load = () => {
      api.listReminders().then((r) => live && setReminders(r.filter((x) => x.status === "pending"))).catch(() => live && setReminders(null));
      api.listTasks("open").then((t) => live && setTasks(t)).catch(() => live && setTasks(null));
    };
    load();
    const id = setInterval(load, 30000);
    return () => {
      live = false;
      clearInterval(id);
    };
  }, [backend]);
  if (backend !== "ready") return null;

  const next = (reminders ?? []).slice().sort((a, b) => a.dueAt.localeCompare(b.dueAt)).slice(0, 3);
  const todo = tasks === null ? "To-do list" : tasks.length === 0 ? "Nothing to do" : `${tasks.length} thing${tasks.length === 1 ? "" : "s"} to do`;
  return (
    <Section
      title="Coming up"
      aside={
        <Link to="/today" className="m-link flex min-h-9 items-center px-1 text-sm">
          See all
        </Link>
      }
    >
      <Group>
        {next.map((r) => (
          <Row key={r.id} icon={<Bell className="size-[18px]" />} title={r.title} value={whenText(r.dueAt)} />
        ))}
        {next.length === 0 && <Row title={<span className="m-muted font-normal">{reminders === null ? "—" : "No reminders"}</span>} />}
        <Row title={todo} to="/today" />
      </Group>
    </Section>
  );
}
