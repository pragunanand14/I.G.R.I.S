type Tone = "ok" | "warn" | "error" | "idle";

const TONES: Record<Tone, string> = {
  ok: "bg-success",
  warn: "bg-warning",
  error: "bg-danger",
  idle: "bg-faint",
};

export function StatusDot({ tone, pulse = false }: { tone: Tone; pulse?: boolean }) {
  return (
    <span className="relative inline-flex size-1.5">
      {pulse && <span className={`absolute inset-0 animate-ping rounded-full opacity-60 ${TONES[tone]}`} />}
      <span className={`relative inline-flex size-1.5 rounded-full ${TONES[tone]}`} />
    </span>
  );
}
