import { Loader2, Mic, Square, Volume2 } from "lucide-react";
import { useVoiceStore } from "@/stores/voiceStore";
import { isMobilePlatform } from "@/services/platform";
import { VOICE_HOTKEY_LABEL } from "@/types/voice";

/** Push-to-talk button: idle → listening → (auto-stops) → transcribing. Click while IGRIS speaks to interrupt. */
export function MicButton({
  disabled,
  large = false,
}: {
  disabled?: boolean;
  large?: boolean;
}) {
  const phase = useVoiceStore((s) => s.phase);
  const level = useVoiceStore((s) => s.level);
  // Subscribe to status so availability updates once it loads.
  useVoiceStore((s) => s.status);
  const can = useVoiceStore.getState().canListen();
  const toggle = useVoiceStore((s) => s.toggle);
  const wakeActive = useVoiceStore((s) => s.wakeActive);

  const unavailable = !can.ok;
  const label =
    phase === "listening"
      ? "Stop listening"
      : phase === "transcribing"
        ? "Transcribing…"
        : phase === "speaking"
          ? "Interrupt and speak"
          : unavailable
            ? (can.reason ?? "Voice input unavailable")
            : `Speak${isMobilePlatform() ? "" : ` (${VOICE_HOTKEY_LABEL})`}${wakeActive ? " — also listening for “IGRIS”" : ""}`;

  return (
    <button
      type="button"
      onClick={() => void toggle()}
      disabled={
        (disabled && phase === "idle") ||
        phase === "transcribing" ||
        (unavailable && phase === "idle")
      }
      aria-label={label}
      title={label}
      aria-pressed={phase === "listening"}
      className={`relative grid shrink-0 place-items-center transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${large ? "size-11 rounded-full" : "size-9 rounded-xl"} ${
        phase === "listening"
          ? "bg-danger text-white"
          : phase === "speaking"
            ? "bg-accent/20 text-accent"
            : large
              ? "bg-accent/15 text-accent active:bg-accent/25"
              : "text-muted hover:bg-surface-hover hover:text-fg"
      }`}
    >
      {phase === "listening" && (
        <span
          className={`pointer-events-none absolute inset-0 border-2 border-danger ${large ? "rounded-full" : "rounded-xl"}`}
          style={{
            transform: `scale(${1 + Math.min(level * 6, 0.6)})`,
            opacity: 0.6,
          }}
        />
      )}
      {wakeActive && phase === "idle" && (
        <span
          aria-hidden
          className="pointer-events-none absolute right-1 top-1 size-1.5 animate-pulse rounded-full bg-accent"
        />
      )}
      {phase === "transcribing" ? (
        <Loader2 className={`${large ? "size-5" : "size-4"} animate-spin`} />
      ) : phase === "speaking" ? (
        <Volume2 className={large ? "size-5" : "size-4"} />
      ) : phase === "listening" ? (
        <Square className={`${large ? "size-4" : "size-3"} fill-current`} />
      ) : (
        <Mic className={large ? "size-[22px]" : "size-4"} />
      )}
    </button>
  );
}
