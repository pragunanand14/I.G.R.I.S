import { useEffect } from "react";
import { useTelemetryPolling } from "@/hooks/useTelemetryPolling";
import { useThemeEffect } from "@/hooks/useThemeEffect";
import { useVoice } from "@/hooks/useVoice";
import { useAppStore } from "@/stores/appStore";
import { useChatStore } from "@/stores/chatStore";
import { useOperatorStore } from "@/stores/operatorStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useTaskStore } from "@/stores/taskStore";

/**
 * App-wide startup and background wiring, shared by the desktop and phone
 * shells: connect to the backend, load settings and AI status, follow
 * operator and task events, apply the theme, sample telemetry and run voice.
 * Returns whether the backend is ready.
 */
export function useAppLifecycle(): boolean {
  const backend = useAppStore((s) => s.backend);
  const initApp = useAppStore((s) => s.init);
  const settings = useSettingsStore((s) => s.settings);
  const loadSettings = useSettingsStore((s) => s.load);

  useEffect(() => {
    void initApp();
  }, [initApp]);

  const loadAiStatus = useChatStore((s) => s.loadAiStatus);
  useEffect(() => {
    if (backend !== "ready") return;
    void loadSettings();
    void loadAiStatus();
  }, [backend, loadSettings, loadAiStatus]);

  const connectOperator = useOperatorStore((s) => s.connect);
  useEffect(() => (backend === "ready" ? connectOperator() : undefined), [backend, connectOperator]);
  const connectTasks = useTaskStore((s) => s.connect);
  useEffect(() => (backend === "ready" ? connectTasks() : undefined), [backend, connectTasks]);

  useThemeEffect(settings);
  useTelemetryPolling(backend === "ready", settings.telemetryIntervalMs);
  useVoice(backend === "ready");

  return backend === "ready";
}
