import { create } from "zustand";

/** The user (or the system) asked for less motion. */
export function reducedMotion(): boolean {
  if (typeof document === "undefined") return true;
  if (document.documentElement.dataset.reducedMotion === "true") return true;
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** Navigation options: animate the change of screen unless motion is reduced. */
export function nav(replace = false) {
  return { viewTransition: !reducedMotion(), replace };
}

/** The opening animation (orb → Home) runs once per app start. */
interface Intro {
  state: "playing" | "done";
  finish: () => void;
}

export const useIntro = create<Intro>((set) => ({
  state: "playing",
  finish: () => set({ state: "done" }),
}));

const THEME_KEY = "igris-theme";

/** Remember the resolved theme so the next start opens in it (no flash before settings load). */
export function rememberTheme() {
  try {
    const t = document.documentElement.dataset.theme;
    if (t === "light" || t === "dark") localStorage.setItem(THEME_KEY, t);
  } catch {
    /* storage unavailable: the default is fine */
  }
}

export function restoreTheme() {
  try {
    const t = localStorage.getItem(THEME_KEY);
    if ((t === "light" || t === "dark") && !document.documentElement.dataset.theme) document.documentElement.dataset.theme = t;
  } catch {
    /* storage unavailable */
  }
}
