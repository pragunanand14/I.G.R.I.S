import "./mobile.css";
import { useEffect } from "react";
import { createHashRouter, Navigate, Outlet, RouterProvider } from "react-router";
import { DevicePrompts } from "@/components/devices/DevicePrompts";
import { ReminderAlerts } from "@/components/productivity/ReminderAlerts";
import { useAppLifecycle } from "@/hooks/useAppLifecycle";
import { useSettingsStore } from "@/stores/settingsStore";
import { rememberTheme, restoreTheme } from "./motion";
import { Opening } from "@/components/core/Opening";
import { ActivityScreen } from "./screens/ActivityScreen";
import { ChatScreen, ChatsScreen } from "./screens/ChatScreen";
import { DevicesScreen } from "./screens/DevicesScreen";
import { HomeScreen } from "./screens/HomeScreen";
import { MemoryScreen } from "./screens/MemoryScreen";
import { AiScreen, AppearanceScreen, MoreScreen } from "./screens/MoreScreen";
import { TodayScreen } from "./screens/TodayScreen";

restoreTheme();

/** The phone layout: one screen at a time, Home in the middle of everything, the opening animation on top. */
function MobileShell() {
  const ready = useAppLifecycle();
  const settings = useSettingsStore((s) => s.settings);
  useEffect(() => {
    // After the theme effect has applied the setting.
    const t = setTimeout(rememberTheme, 0);
    return () => clearTimeout(t);
  }, [settings.theme]);
  return (
    <div className="m-app flex h-full flex-col pt-[env(safe-area-inset-top)] pr-[env(safe-area-inset-right)] pl-[env(safe-area-inset-left)]">
      <main className="relative min-h-0 flex-1 overflow-y-auto">
        <Outlet />
      </main>
      <ReminderAlerts enabled={ready} />
      <DevicePrompts enabled={ready} phone />
      <Opening />
    </div>
  );
}

const router = createHashRouter([
  {
    path: "/",
    element: <MobileShell />,
    children: [
      { index: true, element: <HomeScreen /> },
      { path: "chat", element: <ChatScreen /> },
      { path: "chats", element: <ChatsScreen /> },
      { path: "today", element: <TodayScreen /> },
      { path: "settings", element: <MoreScreen /> },
      { path: "settings/ai", element: <AiScreen /> },
      { path: "settings/appearance", element: <AppearanceScreen /> },
      { path: "settings/memory", element: <MemoryScreen /> },
      { path: "settings/activity", element: <ActivityScreen /> },
      { path: "settings/devices", element: <DevicesScreen /> },
      { path: "*", element: <Navigate to="/" replace /> },
    ],
  },
]);

/** IGRIS on phones: the same IGRIS (stores, services, backend), its own screens. */
export function MobileApp() {
  return <RouterProvider router={router} />;
}
