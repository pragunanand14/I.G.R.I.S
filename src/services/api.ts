import type { AiStatus } from "@/types/ai";
import type { AppInfo, PublicConfig, ReloadResult } from "@/types/app";
import type { Conversation, ConversationDetail } from "@/types/chat";
import type { Memory, MemoryKind } from "@/types/memory";
import type { Settings, SettingsPatch } from "@/types/settings";
import type { SystemSnapshot } from "@/types/system";
import type { AppCandidate, AppEntry, AuditEntry, PermissionRule, ToolActivity, ToolInfo } from "@/types/tools";
import { call } from "./backend";

/** All IPC commands in one place — mirrors `generate_handler!` in `src-tauri/src/lib.rs`. */
export const api = {
  getAppInfo: () => call<AppInfo>("get_app_info"),
  getConfigStatus: () => call<PublicConfig>("get_config_status"),
  getSettings: () => call<Settings>("get_settings"),
  updateSettings: (patch: SettingsPatch) => call<Settings>("update_settings", { patch }),
  getSystemSnapshot: () => call<SystemSnapshot>("get_system_snapshot"),
  getAiStatus: () => call<AiStatus>("get_ai_status"),
  reloadConfig: () => call<ReloadResult>("reload_config"),
  listConversations: () => call<Conversation[]>("list_conversations"),
  getConversation: (id: string) => call<ConversationDetail>("get_conversation", { id }),
  renameConversation: (id: string, title: string) => call<Conversation>("rename_conversation", { id, title }),
  deleteConversation: (id: string) => call<void>("delete_conversation", { id }),
  cancelChat: (requestId: string) => call<boolean>("chat_cancel", { requestId }),
  respondToolApproval: (callId: string, approved: boolean) => call<boolean>("respond_tool_approval", { callId, approved }),
  listTools: () => call<ToolInfo[]>("list_tools"),
  listToolAudit: (limit?: number) => call<AuditEntry[]>("list_tool_audit", { limit }),
  getPermissionPolicy: () => call<PermissionRule[]>("get_permission_policy"),
  listApplications: () => call<AppEntry[]>("list_applications"),
  addApplication: (name: string, path: string) => call<AppEntry>("add_application", { name, path }),
  removeApplication: (id: string) => call<void>("remove_application", { id }),
  detectApplications: () => call<AppCandidate[]>("detect_applications"),
  launchApplication: (id: string) => call<ToolActivity>("launch_application", { id }),
  listMemories: (kind?: MemoryKind, query?: string) => call<Memory[]>("list_memories", { kind, query }),
  addMemory: (kind: MemoryKind, content: string) => call<Memory>("add_memory", { kind, content }),
  updateMemory: (id: number, content: string, kind?: MemoryKind) => call<Memory>("update_memory", { id, content, kind }),
  deleteMemory: (id: number) => call<void>("delete_memory", { id }),
};
