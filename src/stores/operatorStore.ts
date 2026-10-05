import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { api } from "@/services/api";
import { BackendError, hasBackend } from "@/services/backend";
import type { CoreState } from "@/types/assistant";
import type { OperatorAction, OperatorPhase, OperatorSnapshot } from "@/types/operator";

interface OperatorStore {
  snapshot: OperatorSnapshot;
  error: string | null;
  /** Subscribe to `operator-state` events; returns the unsubscribe function. */
  connect: () => () => void;
  control: (action: OperatorAction) => Promise<void>;
  /** Apply a snapshot (from an event or a command). */
  apply: (snapshot: OperatorSnapshot) => void;
}

const EMPTY: OperatorSnapshot = { active: false, task: null };

export const useOperatorStore = create<OperatorStore>((set, get) => ({
  snapshot: EMPTY,
  error: null,

  apply: (snapshot) => set({ snapshot, error: null }),

  connect: () => {
    if (!hasBackend()) return () => undefined;
    let unlisten: (() => void) | undefined;
    let closed = false;
    void listen<OperatorSnapshot>("operator-state", (e) => get().apply(e.payload)).then((u) => {
      if (closed) u();
      else unlisten = u;
    });
    void api
      .getOperatorState()
      .then((s) => get().apply(s))
      .catch(() => undefined);
    return () => {
      closed = true;
      unlisten?.();
    };
  },

  control: async (action) => {
    try {
      get().apply(await api.operatorControl(action));
    } catch (err) {
      set({ error: BackendError.from(err).message });
    }
  },
}));

/** The IGRIS core's look for an operator phase. */
export function phaseToCore(phase: OperatorPhase): CoreState {
  switch (phase) {
    case "planning":
      return "planning";
    case "executing":
      return "executing";
    case "verifying":
      return "verifying";
    case "waiting":
    case "paused":
      return "waiting";
    case "success":
      return "success";
    case "error":
      return "error";
    case "stopped":
      return "idle";
  }
}
