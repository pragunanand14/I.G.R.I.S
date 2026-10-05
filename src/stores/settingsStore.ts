import { create } from "zustand";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import type { Settings, SettingsPatch } from "@/types/settings";

/** Used for rendering before settings load, and when no backend exists. Never persisted from here. */
export const DEFAULT_SETTINGS: Settings = {
  userName: "",
  theme: "dark",
  accent: "azure",
  reducedMotion: false,
  telemetryIntervalMs: 2000,
  aiModel: "",
  aiEffort: "medium",
  confirmLowRisk: false,
  memoryEnabled: true,
};

type Status = "idle" | "loading" | "ready" | "error";

interface SettingsStore {
  settings: Settings;
  status: Status;
  error: string | null;
  saving: boolean;
  load: () => Promise<void>;
  /** Persists via the backend; the UI reflects the backend-confirmed result only. */
  update: (patch: SettingsPatch) => Promise<boolean>;
}

export const useSettingsStore = create<SettingsStore>((set) => ({
  settings: DEFAULT_SETTINGS,
  status: "idle",
  error: null,
  saving: false,
  load: async () => {
    set({ status: "loading", error: null });
    try {
      const settings = await api.getSettings();
      set({ settings, status: "ready" });
    } catch (err) {
      set({ status: "error", error: BackendError.from(err).message });
    }
  },
  update: async (patch) => {
    set({ saving: true, error: null });
    try {
      const settings = await api.updateSettings(patch);
      set({ settings, saving: false, status: "ready" });
      return true;
    } catch (err) {
      set({ saving: false, error: BackendError.from(err).message });
      return false;
    }
  },
}));
