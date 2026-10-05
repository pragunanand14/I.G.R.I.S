import { useNavigate } from "react-router";
import { Composer } from "@/components/chat/Composer";
import { AiCore } from "@/components/core/AiCore";
import { TelemetryStrip } from "@/components/system/TelemetryStrip";
import { StatusDot } from "@/components/ui/StatusDot";
import { useCoreState } from "@/hooks/useCoreState";
import { useAppStore } from "@/stores/appStore";
import { useChatStore } from "@/stores/chatStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { CORE_STATE_LABELS } from "@/types/assistant";
import { greetingFor } from "@/utils/format";

export function HomePage() {
  const coreState = useCoreState();
  const backend = useAppStore((s) => s.backend);
  const backendError = useAppStore((s) => s.backendError);
  const userName = useSettingsStore((s) => s.settings.userName);
  const aiStatus = useChatStore((s) => s.aiStatus);
  const streaming = useChatStore((s) => s.streaming);
  const navigate = useNavigate();

  const disabledReason =
    backend !== "ready"
      ? "The IGRIS backend is unavailable."
      : aiStatus && !aiStatus.ready
        ? "Configure an AI provider in Settings to start chatting."
        : streaming
          ? "IGRIS is responding…"
          : undefined;

  const startConversation = async (text: string) => {
    const chat = useChatStore.getState();
    void chat.openConversation(null); // synchronous for a new conversation
    void navigate("/chat");
    return chat.send(text);
  };

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
        <div className="w-full max-w-2xl">
          <Composer onSend={startConversation} onStop={() => undefined} streaming={false} disabled={disabledReason !== undefined} disabledReason={disabledReason} voice={backend === "ready"} />
          {aiStatus?.ready && aiStatus.effectiveModel && (
            <p className="mt-2 text-center font-mono text-[10px] text-faint">
              {aiStatus.provider} · {aiStatus.effectiveModel}
            </p>
          )}
          {aiStatus && !aiStatus.ready && <p className="mt-2 text-center text-[11px] text-warning">{aiStatus.problem}</p>}
        </div>
        <TelemetryStrip />
      </div>
    </div>
  );
}
