import { ArrowUp, BatteryCharging, BatteryMedium, Bell, ChevronRight, ListTodo, Mic, Wifi, WifiOff } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";
import { SUGGESTIONS, useAsk } from "../ask";
import { api } from "@/services/api";
import { useAppStore } from "@/stores/appStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useSystemStore } from "@/stores/systemStore";
import { useVoiceStore } from "@/stores/voiceStore";
import type { Reminder, Task } from "@/types/productivity";
import { greetingFor } from "@/utils/format";
import { useStatus } from "../status";
import { whenText } from "../format";
import { Dot, Section } from "../ui";

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
    <div className="mx-auto w-full max-w-xl px-5 pt-7 pb-8">
      <header className="mb-6">
        <p className="m-muted text-[15px]">{now.toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" })}</p>
        <h1 className="m-title mt-1">
          {greetingFor(now)}
          {userName ? `, ${userName}` : ""}
        </h1>
      </header>

      <div className="space-y-5">
        <StatusCard />

        <section className="m-card p-4">
          <form onSubmit={submit} className="flex items-center gap-3">
            <input
              className="m-input flex-1"
              value={text}
              onChange={(e) => setText(e.target.value)}
              placeholder={status.ready ? "Ask IGRIS anything…" : "IGRIS isn't ready yet"}
              aria-label="Ask IGRIS"
              disabled={!status.ready}
              enterKeyHint="send"
            />
            {text.trim() ? (
              <button type="submit" className="m-mic" aria-label="Send" disabled={!status.ready}>
                <ArrowUp className="size-7" />
              </button>
            ) : (
              <TalkButton disabled={!status.ready} />
            )}
          </form>
          <p className="m-faint mt-3 px-1 text-sm">Type a question, or tap the mic and just talk.</p>
        </section>

        <Section title="Try asking">
          <div className="flex flex-wrap gap-2">
            {SUGGESTIONS.map((s) => (
              <button key={s} type="button" className="m-chip" disabled={!status.ready} onClick={() => void ask(s)}>
                {s}
              </button>
            ))}
          </div>
        </Section>

        <ComingUp />
        <PhoneCard />
      </div>
    </div>
  );
}

function StatusCard() {
  const status = useStatus();
  return (
    <section className="m-card flex items-start gap-3 p-4" role="status">
      <span className="mt-1.5">
        <Dot tone={status.tone} />
      </span>
      <div className="min-w-0 flex-1">
        <p className="font-semibold">{status.title}</p>
        <p className="m-muted mt-0.5 text-sm break-words">{status.detail}</p>
        {status.needsSetup && (
          <Link to="/more" className="m-accent mt-2 inline-flex min-h-11 items-center gap-1 text-[15px] font-semibold">
            How to set it up <ChevronRight className="size-4" />
          </Link>
        )}
      </div>
    </section>
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
      <Mic className="size-7" />
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

  const next = (reminders ?? []).slice().sort((a, b) => a.dueAt.localeCompare(b.dueAt)).slice(0, 2);
  return (
    <Section
      title="Coming up"
      aside={
        <Link to="/today" className="m-accent flex min-h-9 items-center text-sm font-semibold">
          See all
        </Link>
      }
    >
      <div className="m-card">
        {next.length === 0 && (
          <div className="m-row">
            <span className="m-icon-badge">
              <Bell className="size-5" />
            </span>
            <span className="m-muted">{reminders === null ? "—" : "No reminders set."}</span>
          </div>
        )}
        {next.map((r) => (
          <div key={r.id} className="m-row">
            <span className="m-icon-badge">
              <Bell className="size-5" />
            </span>
            <div className="min-w-0 flex-1">
              <div className="truncate font-medium">{r.title}</div>
              <div className="m-muted text-sm">{whenText(r.dueAt)}</div>
            </div>
          </div>
        ))}
        <Link to="/today" className="m-row">
          <span className="m-icon-badge">
            <ListTodo className="size-5" />
          </span>
          <span className="flex-1">{tasks === null ? "To-do list" : tasks.length === 0 ? "Nothing on your to-do list" : `${tasks.length} thing${tasks.length === 1 ? "" : "s"} to do`}</span>
          <ChevronRight className="m-faint size-5" />
        </Link>
      </div>
    </Section>
  );
}

/** Battery and connection, read from the phone. Unknown values say so. */
function PhoneCard() {
  const snapshot = useSystemStore((s) => s.snapshot);
  const battery = snapshot?.battery ?? null;
  const online = snapshot?.network.connectivity;
  const charging = battery?.state === "charging" || battery?.state === "full";
  return (
    <Section title="Your phone">
      <div className="grid grid-cols-2 gap-3">
        <div className="m-card p-4">
          {charging ? <BatteryCharging className="m-accent size-6" /> : <BatteryMedium className="m-accent size-6" />}
          <p className="mt-3 text-2xl font-semibold">{battery ? `${Math.round(battery.percent)}%` : "—"}</p>
          <p className="m-muted text-sm">{battery ? (charging ? "Battery · charging" : "Battery") : "Battery unknown"}</p>
        </div>
        <div className="m-card p-4">
          {online === "offline" ? <WifiOff className="size-6" style={{ color: "var(--m-bad)" }} /> : <Wifi className="m-accent size-6" />}
          <p className="mt-3 text-2xl font-semibold">{online === "online" ? "Online" : online === "offline" ? "Offline" : "—"}</p>
          <p className="m-muted text-sm">Internet</p>
        </div>
      </div>
    </Section>
  );
}
