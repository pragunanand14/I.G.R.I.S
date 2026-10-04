export type Effort = "low" | "medium" | "high";

export interface AiStatus {
  provider: string | null;
  configuredModel: string | null;
  ready: boolean;
  /** Actionable reason the provider can't be used. */
  problem: string | null;
  effectiveModel: string | null;
  effort: Effort;
}
