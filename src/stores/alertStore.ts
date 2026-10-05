import { create } from "zustand";
import type { FiredReminder } from "@/types/productivity";

interface AlertState {
  /** Reminders/timers currently ringing, oldest first. */
  ringing: FiredReminder[];
  /** Bumped whenever reminders change outside a page's own actions (fired, dismissed from an alert). */
  revision: number;
  add: (r: FiredReminder) => void;
  remove: (id: number) => void;
  bump: () => void;
}

export const useAlertStore = create<AlertState>((set) => ({
  ringing: [],
  revision: 0,
  add: (r) =>
    set((s) => ({
      ringing: s.ringing.some((x) => x.id === r.id) ? s.ringing.map((x) => (x.id === r.id ? r : x)) : [...s.ringing, r],
      revision: s.revision + 1,
    })),
  remove: (id) => set((s) => ({ ringing: s.ringing.filter((x) => x.id !== id), revision: s.revision + 1 })),
  bump: () => set((s) => ({ revision: s.revision + 1 })),
}));
