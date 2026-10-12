import { create } from "zustand";
import { api } from "@/services/api";

/**
 * The assistant panel: "Hey IGRIS" (or the assistant gesture) wakes the
 * screen and IGRIS opens as a panel over whatever was on screen, Google
 * Assistant style. On the lock screen it listens, but asks the user to
 * unlock before doing anything with the request.
 */
interface SheetStore {
  open: boolean;
  /** No request sent yet in this panel session (the first one starts a new conversation). */
  fresh: boolean;
  /** When this panel session started (its replies are the ones shown). */
  openedAt: number;
  /** A request is waiting on the unlock prompt (the panel stays up meanwhile). */
  waiting: boolean;
  enter: () => void;
  /** Back to the app that was on screen. */
  close: () => void;
  /** Into the full app (after unlocking). */
  expand: () => Promise<boolean>;
}

const mark = (on: boolean) => {
  if (on) document.documentElement.dataset.sheet = "1";
  else delete document.documentElement.dataset.sheet;
};

export const useSheet = create<SheetStore>((set, get) => ({
  open: false,
  fresh: true,
  openedAt: 0,
  waiting: false,
  enter: () => {
    mark(true);
    set({ open: true, fresh: true, openedAt: Date.now() });
    void api.phoneSheetMode(true).catch(() => undefined);
  },
  close: () => {
    if (!get().open) return;
    mark(false);
    set({ open: false });
    void api.phoneSheetHide().catch(() => undefined);
  },
  expand: async () => {
    const unlocked = await api.phoneUnlock().catch(() => false);
    if (!unlocked) return false;
    mark(false);
    set({ open: false });
    await api.phoneSheetMode(false).catch(() => undefined);
    return true;
  },
}));

// Left with Home/Back while the panel was up: next time IGRIS opens as the normal app.
if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState !== "hidden" || !useSheet.getState().open)
      return;
    mark(false);
    useSheet.setState({ open: false });
    void api.phoneSheetMode(false).catch(() => undefined);
  });
}
