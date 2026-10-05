// Mirrors `src-tauri/src/settings/mod.rs`. Keep in sync.
import type { Effort } from "./ai";

export type Theme = "dark" | "light" | "system";
export type Accent = "azure" | "violet" | "emerald" | "amber";

export interface Settings {
  userName: string;
  theme: Theme;
  accent: Accent;
  reducedMotion: boolean;
  telemetryIntervalMs: number;
  /** Model override; empty = AI_MODEL or provider default. */
  aiModel: string;
  aiEffort: Effort;
  /** Ask before LOW-risk tool actions (sensitive and critical always ask). */
  confirmLowRisk: boolean;
  /** Use and update persistent memory. */
  memoryEnabled: boolean;
}

export type SettingsPatch = Partial<Settings>;

export const USER_NAME_MAX_CHARS = 48;
export const TELEMETRY_INTERVAL_MIN_MS = 1000;
export const TELEMETRY_INTERVAL_MAX_MS = 10000;
export const AI_MODEL_MAX_CHARS = 100;
