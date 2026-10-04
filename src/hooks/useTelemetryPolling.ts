import { useEffect } from "react";
import { useSystemStore } from "@/stores/systemStore";

/**
 * Polls real system telemetry while `enabled`. Pauses while the window is
 * hidden so IGRIS doesn't burn CPU in the background.
 */
export function useTelemetryPolling(enabled: boolean, intervalMs: number) {
  const sample = useSystemStore((s) => s.sample);

  useEffect(() => {
    if (!enabled) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let cancelled = false;

    const tick = async () => {
      if (cancelled) return;
      if (document.visibilityState === "visible") await sample();
      if (!cancelled) timer = setTimeout(tick, intervalMs);
    };
    void tick();

    const onVisible = () => {
      if (document.visibilityState === "visible") {
        clearTimeout(timer);
        void tick();
      }
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      cancelled = true;
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [enabled, intervalMs, sample]);
}
