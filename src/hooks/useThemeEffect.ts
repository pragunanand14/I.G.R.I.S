import { useEffect } from "react";
import type { Settings } from "@/types/settings";

/** Applies theme, accent and motion settings to the document root. */
export function useThemeEffect({ theme, accent, reducedMotion }: Pick<Settings, "theme" | "accent" | "reducedMotion">) {
  useEffect(() => {
    const root = document.documentElement;
    root.dataset.accent = accent;
    root.dataset.reducedMotion = String(reducedMotion);

    if (theme !== "system") {
      root.dataset.theme = theme;
      return;
    }
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const apply = () => (root.dataset.theme = mq.matches ? "light" : "dark");
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme, accent, reducedMotion]);
}
