import { create } from "zustand";
import type { CoreState } from "@/types/assistant";

interface AssistantStore {
  /** Activity state set by the orchestration layer (chat, voice, tools). */
  activity: Exclude<CoreState, "error">;
  setActivity: (activity: Exclude<CoreState, "error">) => void;
}

export const useAssistantStore = create<AssistantStore>((set) => ({
  activity: "idle",
  setActivity: (activity) => set({ activity }),
}));
