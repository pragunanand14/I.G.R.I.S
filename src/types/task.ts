// Mirrors `src-tauri/src/orchestrator/{task,mod}.rs`. Keep in sync.

export type TaskState =
  | "created"
  | "planning"
  | "waiting_for_approval"
  | "executing"
  | "paused"
  | "verifying"
  | "recovering"
  | "completed"
  | "failed"
  | "cancelled"
  /** Handed back without a verified result. */
  | "ended";

export type TaskKind = "general" | "operator";
export type StepStatus = "pending" | "active" | "completed" | "failed" | "skipped";
export type Verification = "passed" | "failed" | "unverified" | "operator" | "not_applicable";

export interface PlanStep {
  title: string;
  status: StepStatus;
}

export interface ActionRecord {
  tool: string;
  target: string;
  ok: boolean;
  verification: Verification;
  note: string;
}

export interface TaskInfo {
  id: string;
  conversationId: string | null;
  kind: TaskKind;
  objective: string;
  state: TaskState;
  plan: PlanStep[];
  currentStep: number | null;
  /** What IGRIS is doing right now. */
  activity: string | null;
  steps: number;
  failures: number;
  result: string | null;
  error: string | null;
  pauseReason: string | null;
  context: { actions: ActionRecord[]; interrupted: boolean; resumes: number };
  project: { id: number; name: string } | null;
  createdAt: string;
  updatedAt: string;
  /** A turn is working on it right now. */
  live: boolean;
}

export interface TaskUpdate {
  event: { type: string };
  task: TaskInfo;
}

export type TaskControl = "pause" | "resume" | "stop";

export const FINAL_STATES: TaskState[] = ["completed", "failed", "cancelled", "ended"];
