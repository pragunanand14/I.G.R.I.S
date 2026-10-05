import { CalendarClock } from "lucide-react";
import type { TimePreview } from "@/hooks/useTimePreview";

/** Shows what a natural-language time resolved to, or why it couldn't be read. */
export function TimeHint({ preview }: { preview: TimePreview }) {
  if (preview.state === "empty") return null;
  if (preview.state === "error") {
    return (
      <p role="status" className="text-[11px] text-danger">
        {preview.message}
      </p>
    );
  }
  return (
    <p role="status" className="flex items-center gap-1 text-[11px] text-muted">
      <CalendarClock className="size-3" /> {preview.parsed.description}
    </p>
  );
}
