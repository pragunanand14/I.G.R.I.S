export interface RoutineRun {
  id: number;
  routineId: number;
  startedAt: string;
  finishedAt: string | null;
  status: "running" | "done" | "failed" | "skipped";
  summary: string | null;
  conversationId: string | null;
}

export interface Routine {
  id: number;
  name: string;
  instruction: string;
  /** In plain words: "Every weekday at 08:00". */
  when: string;
  enabled: boolean;
  nextRunAt: string | null;
  lastRunAt: string | null;
  createdAt: string;
  lastRun: RoutineRun | null;
}
