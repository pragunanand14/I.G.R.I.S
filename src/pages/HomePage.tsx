import { ArrowUp } from "lucide-react";
import { AiCore } from "@/components/core/AiCore";
import { TelemetryStrip } from "@/components/system/TelemetryStrip";
import { StatusDot } from "@/components/ui/StatusDot";
import { useCoreState } from "@/hooks/useCoreState";
import { useAppStore } from "@/stores/appStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { CORE_STATE_LABELS } from "@/types/assistant";
import { greetingFor } from "@/utils/format";

export function HomePage() {
  const coreState = useCoreState();
  const backend = useAppStore((s) => s.backend);
  const backendError = useAppStore((s) => s.backendError);
  const userName = useSettingsStore((s) => s.settings.userName);

  const status =
    backend === "ready"
      ? { text: "Online", tone: "ok" as const }
      : backend === "connecting"
        ? { text: "Initializing", tone: "warn" as const }
        : { text: "Offline", tone: "error" as const };

  return (
    <div className="flex min-h-full flex-col items-center justify-between gap-6 px-8 pt-10 pb-8">
      <div className="flex flex-col items-center gap-2 text-center">
        <h1 className="text-[28px] font-extralight tracking-[0.55em] text-fg pl-[0.55em]">IGRIS</h1>
        <div className="flex items-center gap-2" role="status">
          <StatusDot tone={status.tone} pulse={backend === "connecting"} />
          <span className="text-label">{status.text}</span>
        </div>
      </div>

      <div className="flex flex-col items-center gap-5">
        <AiCore state={coreState} size={300} />
        <div className="flex flex-col items-center gap-1.5 text-center">
          <p className="text-sm font-medium tracking-wide text-muted" aria-live="polite">
            {CORE_STATE_LABELS[coreState]}
          </p>
          {backend === "ready" && (
            <p className="text-xs text-faint">
              {greetingFor(new Date())}
              {userName ? `, ${userName}` : ""}.
            </p>
          )}
          {backendError && <p className="max-w-md text-xs text-danger">{backendError}</p>}
        </div>
      </div>

      <div className="flex w-full flex-col items-center gap-6">
        <form className="w-full max-w-2xl" onSubmit={(e) => e.preventDefault()} aria-describedby="prompt-note">
          <div className="flex items-center gap-2 rounded-2xl border border-line bg-elevated/70 py-2 pr-2 pl-5 shadow-[var(--shadow)] backdrop-blur">
            <input
              disabled
              aria-label="Message IGRIS"
              placeholder="Ask IGRIS anything…"
              className="h-9 min-w-0 flex-1 bg-transparent text-sm text-fg placeholder:text-faint focus:outline-none disabled:cursor-not-allowed"
            />
            <button
              type="submit"
              disabled
              aria-label="Send"
              className="grid size-9 place-items-center rounded-xl bg-surface-strong text-faint disabled:cursor-not-allowed"
            >
              <ArrowUp className="size-4" />
            </button>
          </div>
          <p id="prompt-note" className="mt-2 text-center text-[11px] text-faint">
            Conversation is not connected yet — it arrives with the AI chat layer (Phase 2).
          </p>
        </form>
        <TelemetryStrip />
      </div>
    </div>
  );
}
