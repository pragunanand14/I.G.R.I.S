import type { AuditEntry } from "@/types/tools";

/** Plain-language outcome of one logged action (statuses as the executor writes them). */
export function outcome(e: AuditEntry): { text: string; tone: "ok" | "bad" | "muted" } {
  if (e.approval === "denied" || e.status === "denied") return { text: "You said no", tone: "muted" };
  switch (e.status) {
    case "completed":
      return { text: e.approval === "approved" ? "Done, with your OK" : "Done", tone: "ok" };
    case "cancelled":
      return { text: "Stopped", tone: "muted" };
    case "running":
    case "awaiting_approval":
      return { text: "Not finished", tone: "muted" };
    default:
      return { text: "Didn't work", tone: "bad" };
  }
}
