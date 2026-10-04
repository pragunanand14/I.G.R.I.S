import { Check } from "lucide-react";
import { useState, type ReactNode } from "react";
import { PageHeader } from "@/components/ui/PageHeader";
import { Panel } from "@/components/ui/Panel";
import { Segmented } from "@/components/ui/Segmented";
import { StatusDot } from "@/components/ui/StatusDot";
import { Toggle } from "@/components/ui/Toggle";
import { useAppStore } from "@/stores/appStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { USER_NAME_MAX_CHARS, type Accent, type Theme } from "@/types/settings";

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
  const config = useAppStore((s) => s.config);
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

        <Panel title="AI provider">
          {config ? (
            <>
              <div className="mb-2 flex items-center gap-2 text-sm">
                <StatusDot tone={config.aiProvider && config.aiKeyConfigured ? "ok" : "idle"} />
                <span className="text-fg">
                  {config.aiProvider ? `${config.aiProvider}${config.aiModel ? ` · ${config.aiModel}` : ""}` : "No provider configured"}
                </span>
              </div>
              <InfoRow label="API key" value={config.aiKeyConfigured ? "Configured (hidden)" : "Not set"} />
              <InfoRow label="Web search key" value={config.searchConfigured ? "Configured (hidden)" : "Not set"} />
              <InfoRow label="Loaded .env files" value={config.envFiles.length ? config.envFiles.join(", ") : "None"} />
              <p className="mt-3 text-xs text-muted">
                Secrets are read only by the native backend from environment variables or a <code className="font-mono">.env</code> file
                {info ? (
                  <>
                    {" "}
                    in <code className="font-mono" data-selectable>{info.configDir}</code>
                  </>
                ) : null}
                . They are never sent to the interface. Restart IGRIS after changing them.
              </p>
            </>
          ) : (
            <p className="text-sm text-muted">Unavailable without the desktop backend.</p>
          )}
        </Panel>

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
