/** Visual/behavioural state of the IGRIS core. */
export type CoreState =
  | "idle"
  | "listening"
  | "thinking"
  | "speaking"
  | "planning"
  | "executing"
  | "waiting"
  | "verifying"
  | "success"
  | "error";

export const CORE_STATE_LABELS: Record<CoreState, string> = {
  idle: "Standing by",
  listening: "Listening",
  thinking: "Thinking",
  speaking: "Responding",
  planning: "Planning",
  executing: "Executing",
  waiting: "Waiting",
  verifying: "Verifying",
  success: "Complete",
  error: "Attention required",
};
