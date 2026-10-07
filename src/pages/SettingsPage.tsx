import { Check, RefreshCw } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { PageHeader } from "@/components/ui/PageHeader";
import { Panel } from "@/components/ui/Panel";
import { Segmented } from "@/components/ui/Segmented";
import { StatusDot } from "@/components/ui/StatusDot";
import { Toggle } from "@/components/ui/Toggle";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import { useChatStore } from "@/stores/chatStore";
import { useVoiceStore } from "@/stores/voiceStore";
import { browserRecognitionAvailable } from "@/services/voice/browserSpeech";
import { browserTtsAvailable, browserVoices } from "@/services/voice/speaker";
import { VOICE_HOTKEY_LABEL } from "@/types/voice";
import { useSettingsStore } from "@/stores/settingsStore";
import type { Effort } from "@/types/ai";
import { AI_MODEL_MAX_CHARS, USER_NAME_MAX_CHARS, type Accent, type Theme } from "@/types/settings";

const ACCENTS: { value: Accent; label: string; swatch: string }[] = [
  { value: "azure", label: "Azure", swatch: "rgb(94 184 255)" },
  { value: "violet", label: "Violet", swatch: "rgb(164 139 255)" },
  { value: "emerald", label: "Emerald", swatch: "rgb(69 214 164)" },
  { value: "amber", label: "Amber", swatch: "rgb(242 184 91)" },
];

const INTERVALS = [
  { value: "1000", label: "1 s" },
  { value: "2000", label: "2 s" },
  { value: "5000", label: "5 s" },
  { value: "10000", label: "10 s" },
];

function Field({ label, description, children }: { label: string; description?: string; children: ReactNode }) {
  return (
    <div className="flex flex-col items-stretch gap-3 border-b border-line py-3.5 last:border-0 sm:flex-row sm:items-center sm:justify-between sm:gap-6">
      <div className="min-w-0">
        <div className="text-sm text-fg">{label}</div>
        {description && <div className="mt-0.5 text-xs text-muted">{description}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function InfoRow({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-6 py-1.5 text-sm">
      <span className="shrink-0 text-muted">{label}</span>
      <span className="truncate text-right font-mono text-xs text-fg" data-selectable title={typeof value === "string" ? value : undefined}>
        {value}
      </span>
    </div>
  );
}

export function SettingsPage() {
  const backend = useAppStore((s) => s.backend);
  const info = useAppStore((s) => s.info);
  const { settings, status, error, saving, update } = useSettingsStore();
  const disabled = backend !== "ready" || status !== "ready" || saving;

  return (
    <div className="mx-auto max-w-3xl p-8">
      <PageHeader
        title="Settings"
        description="Preferences are stored locally in the IGRIS database."
        action={saving ? <span className="text-xs text-muted">Saving…</span> : undefined}
      />

      {backend !== "ready" && (
        <p className="mb-4 rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">
          Settings can't be loaded or saved without the IGRIS desktop backend.
        </p>
      )}
      {error && (
        <p role="alert" className="mb-4 rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">
          {error}
        </p>
      )}

      <div className="space-y-4">
        <Panel title="Profile">
          <Field label="Your name" description="How IGRIS addresses you. Leave empty to stay anonymous.">
            {/* Keyed on load status so the draft picks up the stored name once settings load. */}
            <NameField key={status} saved={settings.userName} disabled={disabled} onCommit={(userName) => update({ userName })} />
          </Field>
        </Panel>

        <Panel title="Appearance">
          <Field label="Theme">
            <Segmented<Theme>
              label="Theme"
              value={settings.theme}
              disabled={disabled}
              onChange={(theme) => void update({ theme })}
              options={[
                { value: "dark", label: "Dark" },
                { value: "light", label: "Light" },
                { value: "system", label: "System" },
              ]}
            />
          </Field>
          <Field label="Accent">
            <div role="radiogroup" aria-label="Accent colour" className="flex gap-2">
              {ACCENTS.map((a) => (
                <button
                  key={a.value}
                  type="button"
                  role="radio"
                  aria-checked={settings.accent === a.value}
                  aria-label={a.label}
                  title={a.label}
                  disabled={disabled}
                  onClick={() => void update({ accent: a.value })}
                  className={`size-6 rounded-full border-2 transition-transform hover:scale-110 disabled:opacity-40 ${
                    settings.accent === a.value ? "border-fg" : "border-transparent"
                  }`}
                  style={{ background: a.swatch }}
                />
              ))}
            </div>
          </Field>
          <Field label="Reduce motion" description="Stops core and interface animations. Your OS setting is always respected.">
            <Toggle
              label="Reduce motion"
              checked={settings.reducedMotion}
              disabled={disabled}
              onChange={(reducedMotion) => void update({ reducedMotion })}
            />
          </Field>
        </Panel>

        <Panel title="System">
          <Field label="Telemetry refresh" description="How often CPU, memory, network and battery are sampled.">
            <Segmented
              label="Telemetry refresh interval"
              value={String(settings.telemetryIntervalMs)}
              disabled={disabled}
              onChange={(v) => void update({ telemetryIntervalMs: Number(v) })}
              options={INTERVALS}
            />
          </Field>
        </Panel>

        <AiPanel disabled={disabled} configDir={info?.configDir ?? null} />

        <VoicePanel disabled={disabled} />
        <OperatorPanel disabled={disabled} />

        <Panel title="About">
          {info ? (
            <>
              <InfoRow label="Version" value={`${info.version}${info.debug ? " (debug)" : ""}`} />
              <InfoRow label="Platform" value={`${info.platform} · ${info.arch}`} />
              <InfoRow label="Database" value={info.dbPath} />
              <InfoRow label="Schema version" value={String(info.schemaVersion)} />
              <InfoRow label="Logs" value={info.logDir} />
            </>
          ) : (
            <p className="text-sm text-muted">Unavailable without the desktop backend.</p>
          )}
        </Panel>
      </div>
    </div>
  );
}

function NameField({
  saved,
  disabled,
  onCommit,
}: {
  saved: string;
  disabled: boolean;
  onCommit: (name: string) => Promise<boolean>;
}) {
  const [draft, setDraft] = useState(saved);
  const [flash, setFlash] = useState(false);

  const commit = async () => {
    const next = draft.trim();
    if (next === saved) return setDraft(next);
    if (await onCommit(next)) {
      setDraft(next);
      setFlash(true);
      setTimeout(() => setFlash(false), 1500);
    }
  };

  return (
    <div className="flex items-center gap-2">
      {flash && <Check className="size-4 text-success" aria-label="Saved" />}
      <input
        value={draft}
        maxLength={USER_NAME_MAX_CHARS}
        disabled={disabled}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => void commit()}
        onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
        aria-label="Your name"
        placeholder="Not set"
        className="h-8 w-56 rounded-lg border border-line bg-surface px-3 text-sm text-fg placeholder:text-faint focus:border-accent focus:outline-none disabled:opacity-40"
      />
    </div>
  );
}

function AiPanel({ disabled, configDir }: { disabled: boolean; configDir: string | null }) {
  const config = useAppStore((s) => s.config);
  const setConfig = useAppStore((s) => s.setConfig);
  const aiStatus = useChatStore((s) => s.aiStatus);
  const setAiStatus = useChatStore((s) => s.setAiStatus);
  const { settings, status, update } = useSettingsStore();
  const [reloading, setReloading] = useState(false);
  const [reloadMsg, setReloadMsg] = useState<{ ok: boolean; text: string } | null>(null);

  const reload = async () => {
    setReloading(true);
    setReloadMsg(null);
    try {
      const r = await api.reloadConfig();
      setConfig(r.config);
      setAiStatus(r.ai);
      setReloadMsg(r.ai.ready ? { ok: true, text: "Configuration reloaded. AI is ready." } : { ok: false, text: r.ai.problem ?? "AI is not ready." });
    } catch (err) {
      setReloadMsg({ ok: false, text: BackendError.from(err).message });
    } finally {
      setReloading(false);
    }
  };

  const commitModel = async (value: string) => {
    if (value.trim() === settings.aiModel) return;
    if (await update({ aiModel: value })) {
      const fresh = await api.getAiStatus().catch(() => null);
      if (fresh) setAiStatus(fresh);
    }
  };

  return (
    <Panel
      title="AI"
      action={
        <button
          type="button"
          onClick={() => void reload()}
          disabled={reloading || !config}
          className="flex items-center gap-1.5 rounded-md px-2 py-1 text-xs text-muted hover:bg-surface-hover hover:text-fg disabled:opacity-40"
        >
          <RefreshCw className={`size-3.5 ${reloading ? "animate-spin" : ""}`} />
          Reload configuration
        </button>
      }
    >
      {aiStatus && config ? (
        <>
          <div className="mb-3 flex items-start gap-2 text-sm">
            <span className="mt-1.5">
              <StatusDot tone={aiStatus.ready ? "ok" : "warn"} />
            </span>
            <div>
              <div className="text-fg">
                {aiStatus.ready ? `Ready — ${aiStatus.provider} · ${aiStatus.effectiveModel ?? "no model"}` : "Not ready"}
              </div>
              {aiStatus.problem && <div className="mt-0.5 text-xs text-warning">{aiStatus.problem}</div>}
            </div>
          </div>
          {reloadMsg && <p className={`mb-3 text-xs ${reloadMsg.ok ? "text-success" : "text-warning"}`}>{reloadMsg.text}</p>}

          <Field label="Model override" description={`Leave empty to use ${aiStatus.configuredModel ? `the configured model (${aiStatus.configuredModel})` : "AI_MODEL"}.`}>
            <ModelField key={`${status}-${settings.aiModel}`} saved={settings.aiModel} disabled={disabled} onCommit={commitModel} placeholder={aiStatus.configuredModel ?? "model id"} />
          </Field>
          <Field label="Response depth" description="How much the model reasons before answering, on models that support it (Claude, Gemini 2.5+, OpenAI reasoning models). Medium uses each provider's default; higher is slower and costs more.">
            <Segmented<Effort>
              label="Response depth"
              value={settings.aiEffort}
              disabled={disabled}
              onChange={(aiEffort) => void update({ aiEffort })}
              options={[
                { value: "low", label: "Fast" },
                { value: "medium", label: "Balanced" },
                { value: "high", label: "Deep" },
              ]}
            />
          </Field>

          <div className="mt-2 border-t border-line pt-3">
            <InfoRow label="Provider" value={config.aiProvider ?? "Not set"} />
            <InfoRow label="API key" value={config.aiKeyConfigured ? "Configured (hidden)" : "Not set"} />
            <InfoRow
              label="Web search"
              value={
                config.webSearch === "anthropic"
                  ? "Anthropic built-in (billed per search)"
                  : config.webSearch === "brave"
                    ? "Brave Search"
                    : config.webSearch === "tavily"
                      ? "Tavily"
                      : "Not available — set SEARCH_PROVIDER + SEARCH_API_KEY"
              }
            />
            {config.aiBaseUrl && <InfoRow label="Endpoint" value={config.aiBaseUrl} />}
            <InfoRow label="Loaded .env files" value={config.envFiles.length ? config.envFiles.join(", ") : "None"} />
          </div>
          <p className="mt-3 text-xs text-muted">
            Set <code className="font-mono">AI_PROVIDER</code> (anthropic, openai, gemini or local), <code className="font-mono">AI_API_KEY</code> and optionally{" "}
            <code className="font-mono">AI_MODEL</code> in a <code className="font-mono">.env</code> file
            {configDir ? (
              <>
                {" "}
                in <code className="font-mono" data-selectable>{configDir}</code>
              </>
            ) : null}
            , then reload. Keys are read only by the native backend and never shown here.
          </p>
        </>
      ) : (
        <p className="text-sm text-muted">Unavailable without the desktop backend.</p>
      )}
    </Panel>
  );
}

function ModelField({
  saved,
  disabled,
  onCommit,
  placeholder,
}: {
  saved: string;
  disabled: boolean;
  onCommit: (v: string) => Promise<void>;
  placeholder: string;
}) {
  const [draft, setDraft] = useState(saved);
  return (
    <input
      value={draft}
      maxLength={AI_MODEL_MAX_CHARS}
      disabled={disabled}
      spellCheck={false}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => void onCommit(draft)}
      onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
      aria-label="Model override"
      placeholder={placeholder}
      className="h-8 w-56 rounded-lg border border-line bg-surface px-3 font-mono text-xs text-fg placeholder:text-faint focus:border-accent focus:outline-none disabled:opacity-40"
    />
  );
}

const MODE_LABEL: Record<string, string> = {
  browser: "Built-in (this system)",
  groq: "Groq",
  gemini: "Gemini",
  openai: "OpenAI-compatible",
  local: "Local server",
};

function VoicePanel({ disabled }: { disabled: boolean }) {
  const { settings, update } = useSettingsStore();
  const status = useVoiceStore((s) => s.status);
  const voiceError = useVoiceStore((s) => s.error);
  const phase = useVoiceStore((s) => s.phase);
  const say = useVoiceStore((s) => s.say);
  const [voices, setVoices] = useState<SpeechSynthesisVoice[]>(() => browserVoices());

  useEffect(() => {
    if (!browserTtsAvailable()) return;
    const refresh = () => setVoices(browserVoices());
    window.speechSynthesis.addEventListener?.("voiceschanged", refresh);
    return () => window.speechSynthesis.removeEventListener?.("voiceschanged", refresh);
  }, []);

  const recognition = browserRecognitionAvailable();

  // Always-on listening uses the configured speech service (the webview recogniser rarely works in WebView2).

  const wakeService = !!status && status.stt.mode !== "browser" && !status.stt.problem;

  const wakeAvailable = wakeService || recognition;
  const sttText = !status
    ? "Unavailable without the desktop backend"
    : status.stt.problem
      ? status.stt.problem
      : status.stt.mode === "browser"
        ? recognition
          ? MODE_LABEL.browser
          : "Not available here — set STT_PROVIDER=gemini, openai or local"
        : MODE_LABEL[status.stt.mode] ?? status.stt.mode;
  const ttsText = !status
    ? "—"
    : status.tts.problem
      ? status.tts.problem
      : status.tts.mode === "browser"
        ? browserTtsAvailable()
          ? `${MODE_LABEL.browser} · ${voices.length} voice${voices.length === 1 ? "" : "s"}`
          : "No system voices available"
        : MODE_LABEL[status.tts.mode] ?? status.tts.mode;

  return (
    <Panel title="Voice">
      <InfoRow label="Speech recognition" value={sttText} />
      <InfoRow label="Speech output" value={ttsText} />
      <InfoRow label="Push-to-talk hotkey" value={VOICE_HOTKEY_LABEL} />
      {status?.tts.mode === "browser" && !status.tts.problem && (
        <p className="mt-2 text-xs text-muted">
          For a natural, human-sounding voice set <code className="font-mono">TTS_PROVIDER=groq</code> (uses your Groq key; voices: troy,
          daniel, austin, hannah, diana, autumn) or <code className="font-mono">TTS_PROVIDER=gemini</code> in .env, then reload the
          configuration.
        </p>
      )}
      {voiceError && <p className="mt-2 text-xs text-warning">{voiceError}</p>}

      <div className="mt-2">
        <Field label="Speak replies" description="Read answers aloud when you asked by voice. Talk over IGRIS, press Esc or the mic to interrupt.">
          <Toggle label="Speak replies" checked={settings.voiceAutoSpeak} disabled={disabled} onChange={(voiceAutoSpeak) => void update({ voiceAutoSpeak })} />
        </Field>
        {status?.tts.mode === "browser" && voices.length > 0 && (
          <Field label="Voice">
            <div className="flex items-center gap-2">
              <select
                value={settings.ttsVoice}
                disabled={disabled}
                onChange={(e) => void update({ ttsVoice: e.target.value })}
                aria-label="Voice"
                className="h-8 max-w-56 rounded-lg border border-line bg-surface px-2 text-xs text-fg focus:border-accent focus:outline-none"
              >
                <option value="">Most natural available</option>
                {voices.map((v) => (
                  <option key={v.name} value={v.name}>
                    {v.name} ({v.lang})
                  </option>
                ))}
              </select>
            </div>
          </Field>
        )}
        <Field label="Test voice">
          <button
            type="button"
            disabled={phase === "speaking"}
            onClick={() => void say("Hello. I'm IGRIS, and this is how I sound.")}
            className="rounded-lg border border-line px-3 py-1 text-xs text-fg hover:bg-surface-hover disabled:opacity-40"
          >
            {phase === "speaking" ? "Speaking…" : "Play sample"}
          </button>
        </Field>
        <Field
          label="Always listen for “IGRIS”"
          description={
            wakeService
              ? "Hands-free: say “IGRIS, …”, “Hey IGRIS …” or “Wake up IGRIS”. While on, every phrase spoken near the mic is sent to your speech service to check for the name (uses its quota). Works while IGRIS is open or minimised."
              : recognition
                ? "Hands-free with the system speech recognizer while IGRIS is open. Say “IGRIS, …”."
                : "Needs a speech service: set STT_PROVIDER (gemini, openai or local) in .env."
          }
        >
          <Toggle
            label="Wake word"
            checked={settings.wakeWordEnabled && wakeAvailable}
            disabled={disabled || !wakeAvailable}
            onChange={(wakeWordEnabled) => void update({ wakeWordEnabled })}
          />
        </Field>
      </div>
    </Panel>
  );
}

/** Operator mode: how to stop IGRIS while it operates the computer. */
function OperatorPanel({ disabled }: { disabled: boolean }) {
  const saved = useSettingsStore((s) => s.settings.operatorStopHotkey);
  const status = useSettingsStore((s) => s.status);
  const update = useSettingsStore((s) => s.update);
  return (
    <Panel title="Operator mode">
      <p className="mb-2 text-xs text-muted">
        When a task needs other apps, IGRIS asks to take control of the mouse, keyboard and screen. While it works, a glowing border and a
        floating orb show it&apos;s in control; screenshots go to your AI provider and are never saved. Sending, posting, buying and deleting
        always ask first. Using the computer yourself makes IGRIS look again before its next action; switching windows pauses it.
      </p>
      <Field label="Stop shortcut" description="Global shortcut that stops IGRIS immediately (only active while it operates). Examples: Escape, Ctrl+Shift+X, Pause.">
        <HotkeyField key={`${status}:${saved}`} saved={saved} disabled={disabled} onCommit={(operatorStopHotkey) => update({ operatorStopHotkey })} />
      </Field>
    </Panel>
  );
}

function HotkeyField({ saved, disabled, onCommit }: { saved: string; disabled: boolean; onCommit: (v: string) => Promise<boolean> }) {
  const [draft, setDraft] = useState(saved);
  const commit = async () => {
    const next = draft.trim();
    if (!next || next === saved) return setDraft(saved);
    if (!(await onCommit(next))) setDraft(saved);
  };
  return (
    <input
      value={draft}
      maxLength={40}
      disabled={disabled}
      aria-label="Stop shortcut"
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => void commit()}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
      }}
      className="h-8 w-44 rounded-lg border border-line bg-surface px-2.5 font-mono text-xs text-fg focus:border-accent focus:outline-none"
    />
  );
}
