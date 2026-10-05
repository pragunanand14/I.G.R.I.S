import { Ban, Check, CircleSlash, Loader2, ShieldQuestion, TriangleAlert, X } from "lucide-react";
import { PermissionBadge } from "@/components/ui/PermissionBadge";
import { StoredImage } from "@/components/attachments/AttachmentView";
import { SourceLinks } from "./SourceLinks";
import type { ToolActivity } from "@/types/tools";

const ICONS = {
  running: <Loader2 className="size-3.5 animate-spin text-accent" />,
  awaitingApproval: <ShieldQuestion className="size-3.5 text-warning" />,
  completed: <Check className="size-3.5 text-success" />,
  failed: <TriangleAlert className="size-3.5 text-danger" />,
  invalid: <CircleSlash className="size-3.5 text-danger" />,
  denied: <X className="size-3.5 text-muted" />,
  cancelled: <Ban className="size-3.5 text-muted" />,
} as const;

const STATUS_TEXT = {
  running: "Running…",
  awaitingApproval: "Waiting for your approval",
  completed: "",
  failed: "Failed",
  invalid: "Rejected",
  denied: "Denied",
  cancelled: "Cancelled",
} as const;

interface Props {
  activities: ToolActivity[];
  /** Present only while the turn is live; enables approval buttons. */
  onAnswer?: (callId: string, approved: boolean) => void;
}

/** Shows what IGRIS did (or tried to do) while answering. */
export function ToolActivityList({ activities, onAnswer }: Props) {
  if (activities.length === 0) return null;
  return (
    <ul className="space-y-1.5" aria-label="Tool activity">
      {activities.map((a) =>
        a.status === "awaitingApproval" && onAnswer ? (
          <li key={a.id} className="rounded-lg border border-warning/40 bg-warning/10 px-3 py-2.5" role="alertdialog" aria-label={`Approve: ${a.description}`}>
            <div className="flex items-center gap-2 text-sm text-fg">
              <ShieldQuestion className="size-4 shrink-0 text-warning" />
              <span className="min-w-0 flex-1 font-medium">{a.description}</span>
              {a.permission && <PermissionBadge level={a.permission} />}
            </div>
            <p className="mt-1 pl-6 text-xs text-muted">IGRIS wants to do this. Nothing happens unless you allow it.</p>
            <div className="mt-2 flex gap-2 pl-6">
              <button
                type="button"
                onClick={() => onAnswer(a.id, true)}
                className="rounded-md bg-accent px-3 py-1 text-xs font-semibold text-bg hover:opacity-90"
              >
                Allow
              </button>
              <button
                type="button"
                onClick={() => onAnswer(a.id, false)}
                className="rounded-md border border-line-strong px-3 py-1 text-xs text-fg hover:bg-surface-hover"
              >
                Deny
              </button>
            </div>
          </li>
        ) : (
          <li key={a.id} className="rounded-lg border border-line bg-surface px-3 py-1.5 text-xs">
            <div className="flex items-center gap-2">
            {ICONS[a.status]}
            <span className="min-w-0 truncate text-fg" title={a.tool}>
              {a.description}
            </span>
            {(a.result || STATUS_TEXT[a.status]) && (
              <span className={`min-w-0 flex-1 truncate ${a.status === "completed" ? "text-muted" : "text-faint"}`} title={a.result ?? undefined}>
                — {a.status === "completed" ? a.result : (a.result ?? STATUS_TEXT[a.status])}
              </span>
            )}
            {a.durationMs != null && <span className="ml-auto shrink-0 font-mono text-[10px] text-faint">{a.durationMs} ms</span>}
            </div>
            {a.sources && a.sources.length > 0 && <SourceLinks sources={a.sources} />}
            {a.attachments && a.attachments.length > 0 && (
              <div className="mt-1.5 flex flex-wrap gap-2">
                {a.attachments.map((id) => (
                  <StoredImage key={id} id={id} name="Screenshot" className="h-20" />
                ))}
              </div>
            )}
          </li>
        ),
      )}
    </ul>
  );
}
