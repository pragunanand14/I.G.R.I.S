import { Bot, Brain, ChevronRight, History, Info, Laptop, Mic, Palette, ShieldCheck, Smartphone, User } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import { useChatStore } from "@/stores/chatStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useSystemStore } from "@/stores/systemStore";
import { useVoiceStore } from "@/stores/voiceStore";
import type { Accent, Theme } from "@/types/settings";
import { useStatus } from "../status";
import { Dot, Screen, Section, Switch } from "../ui";

const THEMES: { value: Theme; label: string }[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "Automatic" },
];

const ACCENTS: { value: Accent; label: string; color: string }[] = [
  { value: "azure", label: "Blue", color: "rgb(94 184 255)" },
  { value: "violet", label: "Purple", color: "rgb(164 139 255)" },
  { value: "emerald", label: "Green", color: "rgb(69 214 164)" },
  { value: "amber", label: "Amber", color: "rgb(242 184 91)" },
];

export function MoreScreen() {
  const ready = useAppStore((s) => s.backend) === "ready";
  const settings = useSettingsStore((s) => s.settings);
  const update = useSettingsStore((s) => s.update);
  const error = useSettingsStore((s) => s.error);

  return (
    <Screen title="More">
      {error && (
        <p role="alert" className="text-sm" style={{ color: "var(--m-bad)" }}>
          {error}
        </p>
      )}

      <Section title="You">
        <div className="m-card">
          <NameRow key={settings.userName} disabled={!ready} />
        </div>
      </Section>

      <AiSection />

      <Section title="Your devices">
        <div className="m-card">
          <Link to="/more/devices" className="m-row">
            <span className="m-icon-badge">
              <Laptop className="size-5" />
            </span>
            <div className="min-w-0 flex-1">
              <div className="font-medium">Phone and computer</div>
              <div className="m-muted text-sm">Pair with your PC, send it tasks, approve its actions</div>
            </div>
            <ChevronRight className="m-faint size-5" />
          </Link>
        </div>
      </Section>

      <Section title="Appearance">
        <div className="m-card space-y-4 p-4">
          <div className="flex items-center gap-3">
            <span className="m-icon-badge">
              <Palette className="size-5" />
            </span>
            <span className="font-medium">Look</span>
          </div>
          <div className="grid grid-cols-3 gap-2" role="radiogroup" aria-label="Theme">
            {THEMES.map((t) => (
              <button
                key={t.value}
                type="button"
                role="radio"
                aria-checked={settings.theme === t.value}
                disabled={!ready}
                onClick={() => void update({ theme: t.value })}
                className="m-button px-2"
                style={settings.theme === t.value ? { background: "var(--m-accent-soft)", color: "var(--m-accent)", outline: "2px solid var(--m-accent)" } : undefined}
              >
                {t.label}
              </button>
            ))}
          </div>
          <div className="flex justify-between gap-2" role="radiogroup" aria-label="Colour">
            {ACCENTS.map((a) => (
              <button
                key={a.value}
                type="button"
                role="radio"
                aria-checked={settings.accent === a.value}
                aria-label={a.label}
                disabled={!ready}
                onClick={() => void update({ accent: a.value })}
                className="flex flex-1 flex-col items-center gap-1 py-1 text-sm disabled:opacity-40"
              >
                <span
                  className="size-10 rounded-full"
                  style={{ background: a.color, outline: settings.accent === a.value ? "3px solid var(--m-text)" : "none", outlineOffset: 2 }}
                />
                <span className="m-muted">{a.label}</span>
              </button>
            ))}
          </div>
        </div>
      </Section>

      <Section title="Privacy">
        <div className="m-card">
          <Link to="/more/memory" className="m-row">
            <span className="m-icon-badge">
              <Brain className="size-5" />
            </span>
            <div className="min-w-0 flex-1">
              <div className="font-medium">What IGRIS remembers</div>
              <div className="m-muted text-sm">See, add or delete memories</div>
            </div>
            <ChevronRight className="m-faint size-5" />
          </Link>
          <Switch
            label="Remember things about me"
            description="Lets IGRIS save facts you share, like your preferences."
            checked={settings.memoryEnabled}
            disabled={!ready}
            onChange={(on) => void update({ memoryEnabled: on })}
          />
          <Switch
            label="Ask before every action"
            description="IGRIS always asks before anything sensitive. Turn this on to approve even small actions, like opening an app."
            checked={settings.confirmLowRisk}
            disabled={!ready}
            onChange={(on) => void update({ confirmLowRisk: on })}
          />
          <Link to="/more/activity" className="m-row">
            <span className="m-icon-badge">
              <History className="size-5" />
            </span>
            <div className="min-w-0 flex-1">
              <div className="font-medium">What IGRIS has done</div>
              <div className="m-muted text-sm">Every action, with your approvals</div>
            </div>
            <ChevronRight className="m-faint size-5" />
          </Link>
        </div>
      </Section>

      <PhoneSection />

      <Section title="About">
        <AboutCard />
      </Section>
    </Screen>
  );
}

function NameRow({ disabled }: { disabled: boolean }) {
  const saved = useSettingsStore((s) => s.settings.userName);
  const update = useSettingsStore((s) => s.update);
  const [name, setName] = useState(saved);
  return (
    <div className="m-row">
      <span className="m-icon-badge">
        <User className="size-5" />
      </span>
      <div className="min-w-0 flex-1">
        <label htmlFor="m-name" className="m-muted text-sm">
          What should IGRIS call you?
        </label>
        <input
          id="m-name"
          className="m-input mt-1"
          value={name}
          maxLength={60}
          disabled={disabled}
          placeholder="Your name"
          onChange={(e) => setName(e.target.value)}
          onBlur={() => name.trim() !== saved && void update({ userName: name.trim() })}
        />
      </div>
    </div>
  );
}

/** AI and voice: what's set up, in plain words, and how to fix it. */
function AiSection() {
  const status = useStatus();
  const config = useAppStore((s) => s.config);
  const setConfig = useAppStore((s) => s.setConfig);
  const aiStatus = useChatStore((s) => s.aiStatus);
  const setAiStatus = useChatStore((s) => s.setAiStatus);
  const voice = useVoiceStore((s) => s.status);
  const configDir = useAppStore((s) => s.info?.configDir ?? null);
  const ready = useAppStore((s) => s.backend) === "ready";
  const [checking, setChecking] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  const recheck = async () => {
    setChecking(true);
    setNote(null);
    try {
      const r = await api.reloadConfig();
      setConfig(r.config);
      setAiStatus(r.ai);
      setNote(r.ai.ready ? "All set — IGRIS is ready." : (r.ai.problem ?? "Still not set up."));
    } catch (err) {
      setNote(BackendError.from(err).message);
    } finally {
      setChecking(false);
    }
  };

  const voiceOk = voice ? !voice.stt.problem : null;
  return (
    <Section title="AI and voice">
      <div className="m-card">
        <div className="m-row">
          <span className="m-icon-badge">
            <Bot className="size-5" />
          </span>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2 font-medium">
              <Dot tone={aiStatus?.ready ? "ok" : status.tone} />
              {aiStatus?.ready ? "AI is connected" : "AI isn't set up yet"}
            </div>
            <div className="m-muted text-sm break-words">
              {aiStatus?.ready
                ? `${config?.aiProvider ?? "Provider"}${aiStatus.effectiveModel ? ` · ${aiStatus.effectiveModel}` : ""}`
                : (aiStatus?.problem ?? status.detail)}
            </div>
          </div>
        </div>
        {!aiStatus?.ready && (
          <div className="m-row block text-sm">
            <p className="m-muted">
              On phones, IGRIS reads its AI key from a settings file{configDir ? " in this folder:" : "."} Setting it up from inside the app isn't
              available yet.
            </p>
            {configDir && (
              <code className="mt-2 block rounded-lg px-2 py-1 text-xs break-all" style={{ background: "var(--m-card-2)" }}>
                {configDir}
              </code>
            )}
          </div>
        )}
        <div className="m-row">
          <span className="m-icon-badge">
            <Mic className="size-5" />
          </span>
          <div className="min-w-0 flex-1">
            <div className="font-medium">Talking to IGRIS</div>
            <div className="m-muted text-sm">{voiceOk === null ? "—" : voiceOk ? "Ready — tap the mic on Home or in a chat." : (voice?.stt.problem ?? "Not available.")}</div>
          </div>
        </div>
        <div className="m-row">
          <button type="button" className="m-button w-full" disabled={!ready || checking} onClick={() => void recheck()}>
            {checking ? "Checking…" : "Check the AI setup again"}
          </button>
        </div>
        {note && <p className="m-row m-muted text-sm">{note}</p>}
      </div>
    </Section>
  );
}

function PhoneSection() {
  const snapshot = useSystemStore((s) => s.snapshot);
  const battery = snapshot?.battery ?? null;
  const os = [snapshot?.host.osName, snapshot?.host.osVersion].filter(Boolean).join(" ");
  return (
    <Section title="This phone">
      <div className="m-card">
        <div className="m-row">
          <span className="m-icon-badge">
            <Smartphone className="size-5" />
          </span>
          <div className="min-w-0 flex-1">
            <div className="font-medium">{os || "—"}</div>
            <div className="m-muted text-sm">
              Battery {battery ? `${Math.round(battery.percent)}%${battery.state === "charging" ? ", charging" : ""}` : "unknown"} · Internet{" "}
              {snapshot ? (snapshot.network.connectivity === "online" ? "on" : snapshot.network.connectivity === "offline" ? "off" : "unknown") : "unknown"}
            </div>
          </div>
        </div>
        <div className="m-row">
          <span className="m-icon-badge">
            <ShieldCheck className="size-5" />
          </span>
          <div className="m-muted min-w-0 flex-1 text-sm">
            IGRIS can open your apps and check battery and network. It can't see your screen, read your files or control other apps on this phone.
          </div>
        </div>
      </div>
    </Section>
  );
}

function AboutCard() {
  const info = useAppStore((s) => s.info);
  return (
    <div className="m-card">
      <div className="m-row">
        <span className="m-icon-badge">
          <Info className="size-5" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="font-medium">IGRIS {info ? `${info.version}${info.debug ? " (test build)" : ""}` : ""}</div>
          <div className="m-muted text-sm">The same IGRIS as on your computer. Your chats and memories are stored on this phone; messages go to your AI provider to be answered.</div>
        </div>
      </div>
    </div>
  );
}
