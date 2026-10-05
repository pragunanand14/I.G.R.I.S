import { useEffect } from "react";
import { Outlet } from "react-router";
import { Sidebar } from "@/components/layout/Sidebar";
import { TitleBar } from "@/components/layout/TitleBar";
import { useTelemetryPolling } from "@/hooks/useTelemetryPolling";
import { useThemeEffect } from "@/hooks/useThemeEffect";
import { useVoice } from "@/hooks/useVoice";
import { useAppStore } from "@/stores/appStore";
import { useChatStore } from "@/stores/chatStore";
import { useSettingsStore } from "@/stores/settingsStore";

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

  useThemeEffect(settings);
  useTelemetryPolling(backend === "ready", settings.telemetryIntervalMs);
  useVoice(backend === "ready");

  return (
    <div className="flex h-full flex-col">
      <TitleBar />
      <div className="flex min-h-0 flex-1">
        <Sidebar />
        <main className="bg-grid relative min-w-0 flex-1 overflow-y-auto">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
