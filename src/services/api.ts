import type { AiStatus } from "@/types/ai";
import type { AppInfo, PublicConfig, ReloadResult } from "@/types/app";
import type { Conversation, ConversationDetail } from "@/types/chat";
import type { Settings, SettingsPatch } from "@/types/settings";
import type { SystemSnapshot } from "@/types/system";
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
};
