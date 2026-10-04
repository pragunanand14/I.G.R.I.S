import type { PermissionLevel } from "@/types/tools";
import { PERMISSION_INFO } from "@/types/tools";

const TONES: Record<PermissionLevel, string> = {
  safe: "border-success/30 text-success",
  low: "border-accent/40 text-accent",
  sensitive: "border-warning/40 text-warning",
  critical: "border-danger/40 text-danger",
};

export function PermissionBadge({ level }: { level: PermissionLevel }) {
  return (
    <span
      title={PERMISSION_INFO[level].summary}
      className={`inline-flex items-center rounded border px-1.5 py-px text-[9px] font-semibold tracking-wider uppercase ${TONES[level]}`}
    >
      {PERMISSION_INFO[level].label}
    </span>
  );
}
