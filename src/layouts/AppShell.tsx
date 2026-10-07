import { Outlet } from "react-router";
import { Sidebar } from "@/components/layout/Sidebar";
import { OperatorBanner } from "@/components/operator/OperatorBanner";
import { TitleBar } from "@/components/layout/TitleBar";
import { ReminderAlerts } from "@/components/productivity/ReminderAlerts";
import { useAppLifecycle } from "@/hooks/useAppLifecycle";

export function AppShell() {
  const ready = useAppLifecycle();

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
      <ReminderAlerts enabled={ready} />
    </div>
  );
}
