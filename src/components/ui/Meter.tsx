interface MeterProps {
  /** 0–100, or null when unknown (renders an empty track). */
  value: number | null;
  label: string;
  className?: string;
  tone?: "accent" | "auto";
}

export function Meter({ value, label, className = "", tone = "auto" }: MeterProps) {
  const pct = value == null ? 0 : Math.min(100, Math.max(0, value));
  const color =
    tone === "auto" && pct >= 90 ? "bg-danger" : tone === "auto" && pct >= 75 ? "bg-warning" : "bg-accent";
  return (
    <div
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={value == null ? undefined : Math.round(pct)}
      className={`h-1 w-full overflow-hidden rounded-full bg-surface-strong ${className}`}
    >
      <div className={`h-full rounded-full ${color} transition-[width] duration-500 ease-out`} style={{ width: `${pct}%` }} />
    </div>
  );
}
