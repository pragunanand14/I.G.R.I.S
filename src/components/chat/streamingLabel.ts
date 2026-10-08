import type { StreamingState } from "@/stores/chatStore";

/** What the chat says while IGRIS works on an answer. */
export function streamingLabel(s: StreamingState, now: number): string {
  const last = s.activities.at(-1);
  if (last?.status === "awaitingApproval") return "Waiting for approval…";
  if (last?.status === "running") return `${last.title}…`;
  // The provider made IGRIS wait: say so, with the countdown, instead of an unexplained "Thinking…".
  if (s.retryAt && s.retryAt > now) {
    const secs = Math.ceil((s.retryAt - now) / 1000);
    return s.rateLimited
      ? `The AI service is limiting requests (too many in a short time) — trying again in ${secs} s`
      : `Couldn't reach the AI service — trying again in ${secs} s`;
  }
  let label = s.phase === "waiting" ? "Thinking…" : "Responding…";
  if (s.phase === "waiting" && s.compacting) label = "Condensing earlier messages…";
  if (s.phase === "waiting" && s.reasoningChars) label = `Reasoning… (~${Math.max(1, Math.round(s.reasoningChars / 5))} words)`;
  // Slow (e.g. local) models: show that it's still working.
  const elapsed = s.startedAt ? Math.floor((now - s.startedAt) / 1000) : 0;
  if (s.phase === "waiting" && elapsed >= 10) label += ` ${Math.floor(elapsed / 60)}:${String(elapsed % 60).padStart(2, "0")}`;
  return label;
}
