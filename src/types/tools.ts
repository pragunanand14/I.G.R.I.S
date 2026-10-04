// Mirrors `src-tauri/src/tools`. Keep in sync.
export type PermissionLevel = "safe" | "low" | "sensitive" | "critical";

export type ActivityStatus = "running" | "awaitingApproval" | "completed" | "failed" | "denied" | "invalid" | "cancelled";

export interface ToolActivity {
  id: string;
  tool: string;
  title: string;
  /** null when the call didn't resolve to a registered tool. */
  permission: PermissionLevel | null;
  description: string;
  status: ActivityStatus;
  result: string | null;
  durationMs: number | null;
  /** Characters (code points) of response text written before this call. */
  textOffset?: number;
}

export interface ToolInfo {
  name: string;
  title: string;
  description: string;
  permission: PermissionLevel;
  requiresApproval: boolean;
}

export interface AppEntry {
  id: string;
  name: string;
  path: string;
  createdAt: string;
}

export interface AppCandidate {
  name: string;
  path: string;
}

export interface AuditEntry {
  id: number;
  conversationId: string | null;
  tool: string;
  permission: PermissionLevel | "unknown";
  actor: "assistant" | "user";
  description: string;
  status: string;
  approval: "auto" | "approved" | "denied" | "expired" | "cancelled";
  result: string | null;
  durationMs: number | null;
  createdAt: string;
}

export interface PermissionRule {
  level: PermissionLevel;
  requiresApproval: boolean;
  configurable: boolean;
}

export const PERMISSION_INFO: Record<PermissionLevel, { label: string; summary: string }> = {
  safe: { label: "Safe", summary: "Read-only. Runs without asking." },
  low: { label: "Low", summary: "Contained local changes, like opening an allowed app." },
  sensitive: { label: "Sensitive", summary: "Always asks for confirmation." },
  critical: { label: "Critical", summary: "Always requires explicit confirmation. Never automatic." },
};
