import type { AppInfo, PublicConfig } from "@/types/app";
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
};
