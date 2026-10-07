import { useAppStore } from "@/stores/appStore";
import { useChatStore } from "@/stores/chatStore";
import type { Tone } from "./ui";

export interface Status {
  tone: Tone;
  title: string;
  detail: string;
  /** IGRIS can take a request right now. */
  ready: boolean;
  /** The AI provider still has to be set up. */
  needsSetup: boolean;
}

/** IGRIS's state in plain words (no jargon, no invented values). */
export function useStatus(): Status {
  const backend = useAppStore((s) => s.backend);
  const backendError = useAppStore((s) => s.backendError);
  const aiStatus = useChatStore((s) => s.aiStatus);
  const busy = useChatStore((s) => s.streaming !== null);
  if (backend === "connecting") return { tone: "warn", title: "Starting up…", detail: "IGRIS is getting ready.", ready: false, needsSetup: false };
  if (backend !== "ready")
    return { tone: "bad", title: "IGRIS isn't running", detail: backendError ?? "The IGRIS engine couldn't be reached.", ready: false, needsSetup: false };
  if (!aiStatus) return { tone: "warn", title: "Checking the AI…", detail: "One moment.", ready: false, needsSetup: false };
  if (!aiStatus.ready)
    return { tone: "warn", title: "Needs an AI provider", detail: aiStatus.problem ?? "No AI provider is set up yet.", ready: false, needsSetup: true };
  if (busy) return { tone: "ok", title: "Working on it…", detail: "IGRIS is answering your last request.", ready: true, needsSetup: false };
  return { tone: "ok", title: "Ready to help", detail: "Ask anything, or tap the mic and talk.", ready: true, needsSetup: false };
}
