import { useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import { useChatStore } from "@/stores/chatStore";
import { useDeviceStore } from "@/stores/deviceStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useSystemStore } from "@/stores/systemStore";
import { useVoiceStore } from "@/stores/voiceStore";
import type { Accent, Theme } from "@/types/settings";
import { useStatus } from "../status";
import { Dot, ErrorText, Group, Row, Screen, Section, Switch } from "../ui";

const THEMES: { value: Theme; label: string }[] = [
  { value: "system", label: "Automatic" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

const ACCENTS: { value: Accent; label: string; color: string }[] = [
  { value: "azure", label: "Blue", color: "rgb(94 184 255)" },
  { value: "violet", label: "Purple", color: "rgb(164 139 255)" },
  { value: "emerald", label: "Green", color: "rgb(69 214 164)" },
  { value: "amber", label: "Amber", color: "rgb(242 184 91)" },
];

/** Settings and everything else, as short lists. */
export function MoreScreen() {
  const ready = useAppStore((s) => s.backend) === "ready";
  const settings = useSettingsStore((s) => s.settings);
  const update = useSettingsStore((s) => s.update);
  const error = useSettingsStore((s) => s.error);
  const aiReady = useChatStore((s) => s.aiStatus?.ready ?? false);
  const paired = useDeviceStore((s) => s.overview?.devices.filter((d) => d.trusted && !d.revokedAt).length ?? 0);

  return (
    <Screen title="Settings" back="/">
      {error && <ErrorText>{error}</ErrorText>}

      <Group>
        <NameRow key={settings.userName} disabled={!ready} />
      </Group>

      <Group>
        <Row title="Phone and computer" value={paired > 0 ? "Paired" : undefined} to="/settings/devices" />
        <Row title="AI and voice" value={aiReady ? "Connected" : "Set up"} to="/settings/ai" />
        <Row title="Appearance" value={THEMES.find((t) => t.value === settings.theme)?.label} to="/settings/appearance" />
      </Group>

      <Section title="Privacy">
        <Group>
          <Switch
            label="Remember things about me"
            description="Save facts you share, like your preferences."
            checked={settings.memoryEnabled}
            disabled={!ready}
            onChange={(on) => void update({ memoryEnabled: on })}
          />
          <Switch
            label="Ask before every action"
            description="Sensitive actions always ask. This also asks for small ones, like opening an app."
            checked={settings.confirmLowRisk}
            disabled={!ready}
            onChange={(on) => void update({ confirmLowRisk: on })}
          />
          <Row title="What IGRIS remembers" to="/settings/memory" />
          <Row title="What IGRIS has done" to="/settings/activity" />
        </Group>
      </Section>

      <PhoneSection />
      <About />
    </Screen>
  );
}

function NameRow({ disabled }: { disabled: boolean }) {
  const saved = useSettingsStore((s) => s.settings.userName);
  const update = useSettingsStore((s) => s.update);
  const [name, setName] = useState(saved);
  return (
    <label className="m-row">
      <span className="font-medium">Your name</span>
      <input
        className="m-field min-h-0 flex-1 text-right"
        value={name}
        maxLength={60}
        disabled={disabled}
        placeholder="What should IGRIS call you?"
        aria-label="What should IGRIS call you?"
        onChange={(e) => setName(e.target.value)}
        onBlur={() => name.trim() !== saved && void update({ userName: name.trim() })}
      />
    </label>
  );
}

function PhoneSection() {
  const snapshot = useSystemStore((s) => s.snapshot);
  const battery = snapshot?.battery ?? null;
  const os = [snapshot?.host.osName, snapshot?.host.osVersion].filter(Boolean).join(" ");
  const internet = snapshot ? (snapshot.network.connectivity === "online" ? "Online" : snapshot.network.connectivity === "offline" ? "Offline" : "Unknown") : "Unknown";
  return (
    <Section title="This phone">
      <Group>
        <Row title="Battery" value={battery ? `${Math.round(battery.percent)}%${battery.state === "charging" ? ", charging" : ""}` : "Unknown"} />
        <Row title="Internet" value={internet} />
        {os && <Row title="System" value={os} />}
      </Group>
      <p className="m-faint px-1 pt-1 text-sm">
        IGRIS can open your apps and check battery and network. It can't see your screen, read your files or control other apps on this phone.
      </p>
    </Section>
  );
}

function About() {
  const info = useAppStore((s) => s.info);
  return (
    <p className="m-faint px-1 text-center text-sm">
      IGRIS {info ? `${info.version}${info.debug ? " (test build)" : ""}` : ""}
      <br />
      Chats and memories are stored on this phone; messages go to your AI provider to be answered.
    </p>
  );
}

/** Settings → AI and voice: what's set up, in plain words, and how to fix it. */
export function AiScreen() {
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
    <Screen title="AI and voice" back="/settings">
      <Group>
        <Row
          title={
            <span className="flex items-center gap-2">
              <Dot tone={aiStatus?.ready ? "ok" : status.tone} />
              {aiStatus?.ready ? "AI is connected" : "AI isn't set up yet"}
            </span>
          }
          detail={
            aiStatus?.ready
              ? `${config?.aiProvider ?? "Provider"}${aiStatus.effectiveModel ? ` · ${aiStatus.effectiveModel}` : ""}`
              : (aiStatus?.problem ?? status.detail)
          }
        />
        <Row
          title="Talking to IGRIS"
          detail={voiceOk === null ? "—" : voiceOk ? "Ready — tap the mic on Home or in a chat." : (voice?.stt.problem ?? "Not available.")}
        />
      </Group>

      {!aiStatus?.ready && (
        <Section title="Setting it up">
          <p className="m-muted px-1 text-[15px]">
            On phones, IGRIS reads its AI key from a settings file{configDir ? " in this folder:" : "."} Setting it up from inside the app isn't available
            yet.
          </p>
          {configDir && (
            <code className="m-card block px-4 py-3 text-xs break-all" data-selectable>
              {configDir}
            </code>
          )}
        </Section>
      )}

      <div className="space-y-3">
        <button type="button" className="m-button w-full" disabled={!ready || checking} onClick={() => void recheck()}>
          {checking ? "Checking…" : "Check the AI setup again"}
        </button>
        {note && <p className="m-muted px-1 text-center text-sm">{note}</p>}
      </div>
    </Screen>
  );
}

/** Settings → Appearance: light/dark and the accent colour. */
export function AppearanceScreen() {
  const ready = useAppStore((s) => s.backend) === "ready";
  const settings = useSettingsStore((s) => s.settings);
  const update = useSettingsStore((s) => s.update);
  return (
    <Screen title="Appearance" back="/settings">
      <Section title="Theme">
        <div className="m-segmented" role="radiogroup" aria-label="Theme">
          {THEMES.map((t) => (
            <button key={t.value} type="button" role="radio" aria-checked={settings.theme === t.value} disabled={!ready} onClick={() => void update({ theme: t.value })}>
              {t.label}
            </button>
          ))}
        </div>
      </Section>
      <Section title="Colour">
        <div className="m-card flex justify-between px-4 py-4" role="radiogroup" aria-label="Colour">
          {ACCENTS.map((a) => (
            <button
              key={a.value}
              type="button"
              role="radio"
              aria-checked={settings.accent === a.value}
              aria-label={a.label}
              disabled={!ready}
              onClick={() => void update({ accent: a.value })}
              className="flex flex-1 flex-col items-center gap-2 text-sm disabled:opacity-40"
            >
              <span
                className="size-9 rounded-full"
                style={{ background: a.color, outline: settings.accent === a.value ? "2px solid var(--m-text)" : "none", outlineOffset: 3 }}
              />
              <span className={settings.accent === a.value ? "" : "m-muted"}>{a.label}</span>
            </button>
          ))}
        </div>
      </Section>
    </Screen>
  );
}
