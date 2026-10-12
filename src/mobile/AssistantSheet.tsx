import { Mic, Square, X } from "lucide-react";
import { useEffect } from "react";
import { useNavigate } from "react-router";
import { useChatStore } from "@/stores/chatStore";
import { useVoiceStore } from "@/stores/voiceStore";
import { useSheet } from "./sheet";

const STATUS: Record<string, string> = {
  listening: "Listening…",
  transcribing: "Got it…",
  thinking: "Thinking…",
  speaking: "",
};

/** The panel IGRIS opens as over other apps: what you said, its answer, approvals, and a way into the full app. */
export function AssistantSheet() {
  const navigate = useNavigate();
  const close = useSheet((s) => s.close);
  const expand = useSheet((s) => s.expand);
  const phase = useVoiceStore((s) => s.phase);
  const level = useVoiceStore((s) => s.level);
  const heard = useVoiceStore((s) => s.lastTranscript);
  const error = useVoiceStore((s) => s.error);
  const replyPending = useVoiceStore((s) => s.replyPending);
  const streaming = useChatStore((s) => s.streaming);
  const messages = useChatStore((s) => s.messages);
  const answerApproval = useChatStore((s) => s.answerApproval);
  const fresh = useSheet((s) => s.fresh);
  const openedAt = useSheet((s) => s.openedAt);

  const lastAssistant = [...messages]
    .reverse()
    .find(
      (m) =>
        m.role === "assistant" && Date.parse(m.createdAt) >= openedAt - 1000,
    );
  const reply = streaming?.text || (!fresh ? lastAssistant?.content : "") || "";
  const approvals =
    streaming?.activities.filter((a) => a.status === "awaitingApproval") ?? [];
  const busy =
    phase !== "idle" || !!streaming || replyPending || approvals.length > 0;

  // Done (answered and spoken, or nothing was said): go back to what was on screen after a moment.
  useEffect(() => {
    if (busy) return;
    const t = setTimeout(close, reply || error ? 9000 : 3500);
    return () => clearTimeout(t);
  }, [busy, reply, error, close]);

  const openApp = async () => {
    if (await expand())
      void navigate(useChatStore.getState().activeId ? "/chat" : "/");
  };

  const status =
    error ??
    (phase === "idle"
      ? streaming
        ? STATUS.thinking
        : ""
      : (STATUS[phase] ?? ""));
  const toggle = () => void useVoiceStore.getState().toggle();

  return (
    <div
      className="m-sheet fixed inset-0 z-[100] flex flex-col justify-end"
      role="dialog"
      aria-label="IGRIS"
    >
      <button
        type="button"
        aria-label="Close IGRIS"
        className="absolute inset-0 bg-black/30"
        onClick={close}
      />
      <div
        className="m-sheet-panel relative mx-2 mb-[max(env(safe-area-inset-bottom),8px)] max-h-[70vh] space-y-3 overflow-y-auto rounded-[28px] px-5 pt-4 pb-5 shadow-2xl"
        style={{ background: "var(--m-surface)" }}
      >
        <div className="flex items-center gap-3">
          <span
            className="m-sheet-orb"
            style={{
              transform: `scale(${1 + (phase === "listening" ? level * 0.6 : 0)})`,
            }}
            data-phase={phase}
            aria-hidden="true"
          />
          <p className="m-muted min-w-0 flex-1 truncate text-sm">
            {status || "IGRIS"}
          </p>
          <button
            type="button"
            className="m-icon-button"
            aria-label="Close"
            onClick={close}
          >
            <X className="size-5" />
          </button>
        </div>
        {heard && (
          <p className="text-[19px] leading-snug font-medium break-words">
            {heard}
          </p>
        )}
        {reply && (
          <p className="m-text max-h-[40vh] overflow-y-auto text-[16px] leading-relaxed break-words whitespace-pre-wrap">
            {reply}
          </p>
        )}
        {approvals.map((a) => (
          <div key={a.id} className="m-card space-y-2 px-3 py-3">
            <p className="text-[15px]">{a.description || a.title}</p>
            <div className="flex gap-2">
              <button
                type="button"
                className="m-button m-button-primary flex-1"
                onClick={() => void answerApproval(a.id, true)}
              >
                Allow
              </button>
              <button
                type="button"
                className="m-button flex-1"
                onClick={() => void answerApproval(a.id, false)}
              >
                Don't
              </button>
            </div>
          </div>
        ))}
        <div className="flex items-center gap-2 pt-1">
          <button
            type="button"
            className="m-button flex-1"
            onClick={() => void openApp()}
          >
            Open IGRIS
          </button>
          <button
            type="button"
            className="m-button m-button-primary flex size-12 items-center justify-center rounded-full p-0"
            aria-label={
              phase === "listening"
                ? "Done"
                : phase === "speaking"
                  ? "Stop"
                  : "Talk"
            }
            onClick={toggle}
          >
            {phase === "listening" || phase === "speaking" ? (
              <Square className="size-5" />
            ) : (
              <Mic className="size-5" />
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
