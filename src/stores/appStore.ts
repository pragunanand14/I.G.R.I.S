import { create } from "zustand";
import { api } from "@/services/api";
import { BackendError, hasBackend } from "@/services/backend";
import type { AppInfo, PublicConfig } from "@/types/app";

export type BackendStatus = "connecting" | "ready" | "unavailable" | "error";

interface AppStore {
  backend: BackendStatus;
  backendError: string | null;
  info: AppInfo | null;
  config: PublicConfig | null;
  init: () => Promise<void>;
}

export const useAppStore = create<AppStore>((set) => ({
  backend: "connecting",
  backendError: null,
  info: null,
  config: null,
  init: async () => {
    if (!hasBackend()) {
      set({
        backend: "unavailable",
        backendError: "Running outside the IGRIS desktop shell — native features are unavailable.",
      });
      return;
    }
    try {
      const [info, config] = await Promise.all([api.getAppInfo(), api.getConfigStatus()]);
      set({ backend: "ready", backendError: null, info, config });
    } catch (err) {
      set({ backend: "error", backendError: BackendError.from(err).message });
    }
  },
}));
