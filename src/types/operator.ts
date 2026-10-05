// Mirrors `src-tauri/src/operator/mod.rs`. Keep in sync.

export type OperatorPhase = "planning" | "executing" | "waiting" | "verifying" | "paused" | "success" | "error" | "stopped";

export type OperatorTaskState =
  | "created"
  | "planning"
  | "waiting_for_permission"
  | "executing"
  | "paused"
  | "verifying"
  | "completed"
  | "failed"
  | "cancelled";

export interface OperatorDisplay {
  id: number;
  name: string;
  rect: { x: number; y: number; w: number; h: number };
  primary: boolean;
}

export interface OperatorTask {
  id: string;
  conversationId: string | null;
  objective: string;
  plan: string[];
  state: OperatorTaskState;
  phase: OperatorPhase;
  /** Short status, e.g. "OPENING VS CODE". */
  status: string;
  steps: number;
  retries: number;
  result: string | null;
  error: string | null;
  pauseReason: string | null;
  display: OperatorDisplay | null;
}

export interface OperatorSnapshot {
  /** IGRIS is controlling (or holding paused control of) the computer. */
  active: boolean;
  task: OperatorTask | null;
}

export type OperatorAction = "pause" | "resume" | "stop";
