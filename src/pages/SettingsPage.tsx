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
    <div className="flex items-center justify-between gap-6 border-b border-line py-3.5 last:border-0">
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
          <Field label="Response depth" description="How much the model reasons before answering (Anthropic models). Higher is slower and costs more.">
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

const MODE_LABEL: Record<string, string> = { browser: "Built-in (this system)", openai: "OpenAI", local: "Local server" };

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
  const sttText = !status
    ? "Unavailable without the desktop backend"
    : status.stt.problem
      ? status.stt.problem
      : status.stt.mode === "browser"
        ? recognition
          ? MODE_LABEL.browser
          : "Not available here — set STT_PROVIDER=openai or local"
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
                <option value="">System default</option>
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
          label="Wake word “IGRIS” (experimental)"
          description={
            recognition
              ? "Listens continuously with the system speech recognizer while IGRIS is open. Say “IGRIS, …”."
              : "Needs built-in speech recognition, which this system's webview doesn't provide."
          }
        >
          <Toggle
            label="Wake word"
            checked={settings.wakeWordEnabled && recognition}
            disabled={disabled || !recognition}
            onChange={(wakeWordEnabled) => void update({ wakeWordEnabled })}
          />
        </Field>
      </div>
    </Panel>
  );
}
