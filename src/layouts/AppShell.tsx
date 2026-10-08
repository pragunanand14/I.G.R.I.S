import { Outlet } from "react-router";
import { Opening } from "@/components/core/Opening";
import { DevicePrompts } from "@/components/devices/DevicePrompts";
import { Sidebar } from "@/components/layout/Sidebar";
import { TitleBar } from "@/components/layout/TitleBar";
import { OperatorBanner } from "@/components/operator/OperatorBanner";
import { ReminderAlerts } from "@/components/productivity/ReminderAlerts";
import { useAppLifecycle } from "@/hooks/useAppLifecycle";
import { rememberTheme, restoreTheme } from "@/hooks/motion";
import { useSettingsStore } from "@/stores/settingsStore";
import { useEffect } from "react";

restoreTheme();

/** The desktop window: the sidebar on the left, the page on a raised sheet on the right, the opening on top. */
export function AppShell() {
  const ready = useAppLifecycle();
  const theme = useSettingsStore((s) => s.settings.theme);
  useEffect(() => {
    const t = setTimeout(rememberTheme, 0);
    return () => clearTimeout(t);
  }, [theme]);

  return (
    <div className="flex h-full bg-bg">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <TitleBar />
        <OperatorBanner />
        <main className="relative mr-2 mb-2 min-h-0 min-w-0 flex-1 overflow-y-auto rounded-2xl border border-line bg-elevated">
          <Outlet />
        </main>
      </div>
      <ReminderAlerts enabled={ready} />
      <DevicePrompts enabled={ready} />
      <Opening />
    </div>
  );
}
