import { create } from "zustand";

const KEY = "igris-wake-charging-only";

function read(): boolean {
  try {
    return localStorage.getItem(KEY) !== "0";
  } catch {
    return true;
  }
}

/** Phone-only: listen for "IGRIS" only while charging (saves battery). On by default. */
export const useWakePrefs = create<{
  chargingOnly: boolean;
  setChargingOnly: (on: boolean) => void;
}>((set) => ({
  chargingOnly: read(),
  setChargingOnly: (on) => {
    try {
      localStorage.setItem(KEY, on ? "1" : "0");
    } catch {
      // Private mode: the choice lasts until IGRIS restarts.
    }
    set({ chargingOnly: on });
  },
}));
