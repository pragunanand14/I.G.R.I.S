import { X } from "lucide-react";
import { useVoiceStore } from "@/stores/voiceStore";

/** One-line voice status under the composer: listening hints and errors. */
export function VoiceNotice() {
  const phase = useVoiceStore((s) => s.phase);
  const error = useVoiceStore((s) => s.error);
  const dismiss = useVoiceStore((s) => s.dismissError);

  if (error) {
    return (
      <p role="alert" className="mt-1.5 flex items-center justify-center gap-1.5 text-[11px] text-warning">
        {error}
        <button type="button" onClick={dismiss} aria-label="Dismiss voice message" className="opacity-70 hover:opacity-100">
          <X className="size-3" />
        </button>
      </p>
    );
  }
  if (phase === "listening") return <p className="mt-1.5 text-center text-[11px] text-danger">Listening… stop talking to send, or press Esc to cancel.</p>;
  if (phase === "transcribing") return <p className="mt-1.5 text-center text-[11px] text-muted">Transcribing…</p>;
  if (phase === "speaking") return <p className="mt-1.5 text-center text-[11px] text-muted">Speaking — talk, press Esc or the mic to interrupt.</p>;
  return null;
}
