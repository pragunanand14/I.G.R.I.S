import { useState } from "react";
import { Link, useNavigate } from "react-router";
import { Composer } from "@/components/chat/Composer";
import { AiCore } from "@/components/core/AiCore";
import { TelemetryStrip } from "@/components/system/TelemetryStrip";
import { StatusDot } from "@/components/ui/StatusDot";
import { nav, useIntro } from "@/hooks/motion";
import { useCoreState } from "@/hooks/useCoreState";
import { useAppStore } from "@/stores/appStore";
import { useChatStore } from "@/stores/chatStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { CORE_STATE_LABELS } from "@/types/assistant";
import { greetingFor } from "@/utils/format";

/** The orb in the middle, a greeting, one place to ask, and the machine at a glance. */
export function HomePage() {
  const coreState = useCoreState();
  const backend = useAppStore((s) => s.backend);
  const backendError = useAppStore((s) => s.backendError);
  const userName = useSettingsStore((s) => s.settings.userName);
  const aiStatus = useChatStore((s) => s.aiStatus);
  const streaming = useChatStore((s) => s.streaming);
  const navigate = useNavigate();
  const intro = useIntro((s) => s.state);
  // Home's pieces rise in only when Home appears at the end of the opening.
  const [entering] = useState(intro === "playing");
  const enter = (i: number, base: string) =>
    entering ? { className: `${base} ${intro === "playing" ? "opacity-0" : "anim-rise"}`, style: { animationDelay: `${i * 80}ms` } } : { className: base };

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
    void navigate("/chat", nav());
    return chat.send(text);
  };

  const status =
    backend === "ready"
      ? { text: "Online", tone: "ok" as const }
      : backend === "connecting"
        ? { text: "Initializing", tone: "warn" as const }
        : { text: "Offline", tone: "error" as const };

  return (
    <div className="flex min-h-full flex-col items-center px-10 pt-6 pb-8">
      <div className="flex w-full flex-1 flex-col items-center justify-center">
        <div data-orb="home" className="aspect-square w-[min(300px,34vh)] [view-transition-name:igris-orb]" style={{ opacity: intro === "playing" ? 0 : 1 }}>
          <AiCore state={coreState} size="100%" />
        </div>

        <div {...enter(0, "mt-8 flex flex-col items-center gap-2 text-center")}>
          <h1 className="text-[32px] leading-tight font-semibold tracking-tight text-fg">
            {greetingFor(new Date())}
            {userName ? `, ${userName}` : ""}
          </h1>
          <div className="flex items-center gap-2 text-sm text-muted" role="status" aria-live="polite">
            <StatusDot tone={status.tone} pulse={backend === "connecting"} />
            <span>{status.text}</span>
            <span className="text-faint">·</span>
            <span key={coreState} className="anim-fade">
              {CORE_STATE_LABELS[coreState]}
            </span>
          </div>
          {backendError && <p className="max-w-md text-xs text-danger">{backendError}</p>}
        </div>

        <div {...enter(1, "mt-9 w-full max-w-2xl")}>
          <Composer
            onSend={startConversation}
            onStop={() => undefined}
            streaming={false}
            disabled={disabledReason !== undefined}
            disabledReason={disabledReason}
            voice={backend === "ready"}
          />
          <div className="mt-3 h-4 text-center text-[11px]">
            {aiStatus?.ready && aiStatus.effectiveModel && (
              <span className="font-mono text-faint">
                {aiStatus.provider} · {aiStatus.effectiveModel}
              </span>
            )}
            {aiStatus && !aiStatus.ready && (
              <span className="text-warning">
                {aiStatus.problem}{" "}
                <Link to="/settings" viewTransition={nav().viewTransition} className="font-medium underline-offset-2 hover:underline">
                  Open Settings
                </Link>
              </span>
            )}
          </div>
        </div>
      </div>

      <div {...enter(2, "mt-6 w-full max-w-4xl")}>
        <TelemetryStrip />
      </div>
    </div>
  );
}
