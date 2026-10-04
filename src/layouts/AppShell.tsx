import { useEffect } from "react";
import { Outlet } from "react-router";
import { Sidebar } from "@/components/layout/Sidebar";
import { TitleBar } from "@/components/layout/TitleBar";
import { useTelemetryPolling } from "@/hooks/useTelemetryPolling";
import { useThemeEffect } from "@/hooks/useThemeEffect";
import { useAppStore } from "@/stores/appStore";
import { useSettingsStore } from "@/stores/settingsStore";

export function AppShell() {
  const backend = useAppStore((s) => s.backend);
  const initApp = useAppStore((s) => s.init);
  const settings = useSettingsStore((s) => s.settings);
  const loadSettings = useSettingsStore((s) => s.load);

  useEffect(() => {
    void initApp();
  }, [initApp]);

  useEffect(() => {
    if (backend === "ready") void loadSettings();
  }, [backend, loadSettings]);

  useThemeEffect(settings);
  useTelemetryPolling(backend === "ready", settings.telemetryIntervalMs);

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
