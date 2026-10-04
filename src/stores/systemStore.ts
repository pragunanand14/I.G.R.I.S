import { create } from "zustand";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import type { SystemSnapshot } from "@/types/system";

export const HISTORY_LENGTH = 60;

interface SystemStore {
  snapshot: SystemSnapshot | null;
  error: string | null;
  /** Recent CPU / memory samples for sparklines (percent, oldest first). */
  cpuHistory: number[];
  memoryHistory: number[];
  sample: () => Promise<void>;
}

function push(history: number[], value: number | null): number[] {
  if (value == null) return history;
  const next = [...history, value];
  return next.length > HISTORY_LENGTH ? next.slice(next.length - HISTORY_LENGTH) : next;
}

export const useSystemStore = create<SystemStore>((set, get) => ({
  snapshot: null,
  error: null,
  cpuHistory: [],
  memoryHistory: [],
  sample: async () => {
    try {
      const snapshot = await api.getSystemSnapshot();
      const { cpuHistory, memoryHistory } = get();
      const mem = snapshot.memory.totalBytes > 0 ? (snapshot.memory.usedBytes / snapshot.memory.totalBytes) * 100 : null;
      set({
        snapshot,
        error: null,
        cpuHistory: push(cpuHistory, snapshot.cpu.usagePercent),
        memoryHistory: push(memoryHistory, mem),
      });
    } catch (err) {
      set({ error: BackendError.from(err).message });
    }
  },
}));
