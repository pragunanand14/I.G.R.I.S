/** Visual/behavioural state of the IGRIS core. */
export type CoreState = "idle" | "listening" | "thinking" | "speaking" | "executing" | "error";

export const CORE_STATE_LABELS: Record<CoreState, string> = {
  idle: "Standing by",
  listening: "Listening",
  thinking: "Thinking",
  speaking: "Responding",
  executing: "Executing",
  error: "Attention required",
};
