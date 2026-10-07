import { useEffect } from "react";
import { Outlet } from "react-router";
import { Sidebar } from "@/components/layout/Sidebar";
import { OperatorBanner } from "@/components/operator/OperatorBanner";
import { TitleBar } from "@/components/layout/TitleBar";
import { ReminderAlerts } from "@/components/productivity/ReminderAlerts";
import { useTelemetryPolling } from "@/hooks/useTelemetryPolling";
import { useThemeEffect } from "@/hooks/useThemeEffect";
import { useVoice } from "@/hooks/useVoice";
import { useAppStore } from "@/stores/appStore";
import { useChatStore } from "@/stores/chatStore";
import { useOperatorStore } from "@/stores/operatorStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useTaskStore } from "@/stores/taskStore";

export function AppShell() {
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

  return (
    <div className="flex h-full flex-col pt-[env(safe-area-inset-top)] pr-[env(safe-area-inset-right)] pb-[env(safe-area-inset-bottom)] pl-[env(safe-area-inset-left)]">
      <TitleBar />
      <OperatorBanner />
      {/* Narrow screens (phones): navigation moves to the bottom. */}
      <div className="flex min-h-0 flex-1 flex-col-reverse md:flex-row">
        <Sidebar />
        <main className="bg-grid relative min-h-0 min-w-0 flex-1 overflow-y-auto">
          <Outlet />
        </main>
      </div>
      <ReminderAlerts enabled={backend === "ready"} />
    </div>
  );
}
