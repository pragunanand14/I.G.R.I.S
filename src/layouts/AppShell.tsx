import { useEffect, useState } from "react";
import { Outlet } from "react-router";
import { Opening } from "@/components/core/Opening";
import { DevicePrompts } from "@/components/devices/DevicePrompts";
import { CommandPalette } from "@/components/layout/CommandPalette";
import { TopBar } from "@/components/layout/TopBar";
import { OperatorBanner } from "@/components/operator/OperatorBanner";
import { ReminderAlerts } from "@/components/productivity/ReminderAlerts";
import { useAppLifecycle } from "@/hooks/useAppLifecycle";
import { rememberTheme, restoreTheme, useIntro } from "@/hooks/motion";
import { useSettingsStore } from "@/stores/settingsStore";

restoreTheme();

/** The desktop window: one bar on top, the page below it, the opening over everything at start. */
export function AppShell() {
  const ready = useAppLifecycle();
  const theme = useSettingsStore((s) => s.settings.theme);
  const intro = useIntro((s) => s.state);
  const [palette, setPalette] = useState(false);

  useEffect(() => {
    const t = setTimeout(rememberTheme, 0);
    return () => clearTimeout(t);
  }, [theme]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((p) => !p);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex h-full flex-col bg-bg">
      <div className={intro === "playing" ? "opacity-0" : "anim-fade"}>
        <TopBar onCommand={() => setPalette(true)} />
      </div>
      <OperatorBanner />
      <main className="relative min-h-0 min-w-0 flex-1 overflow-y-auto">
        <Outlet />
      </main>
      <ReminderAlerts enabled={ready} />
      <DevicePrompts enabled={ready} />
      <CommandPalette open={palette} onClose={() => setPalette(false)} />
      <Opening />
    </div>
  );
}
