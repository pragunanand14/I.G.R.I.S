import "./mobile.css";
import { useEffect } from "react";
import {
  createHashRouter,
  Navigate,
  Outlet,
  RouterProvider,
  useNavigate,
} from "react-router";
import { api } from "@/services/api";
import { useVoiceStore } from "@/stores/voiceStore";
import { DevicePrompts } from "@/components/devices/DevicePrompts";
import { ReminderAlerts } from "@/components/productivity/ReminderAlerts";
import { useAppLifecycle } from "@/hooks/useAppLifecycle";
import { useSettingsStore } from "@/stores/settingsStore";
import { rememberTheme, restoreTheme } from "./motion";
import { Opening } from "@/components/core/Opening";
import { AccountsScreen } from "./screens/AccountsScreen";
import { ActivityScreen } from "./screens/ActivityScreen";
import { ChatScreen, ChatsScreen } from "./screens/ChatScreen";
import { DevicesScreen } from "./screens/DevicesScreen";
import { HomeScreen } from "./screens/HomeScreen";
import { MemoryScreen } from "./screens/MemoryScreen";
import { MessagesScreen } from "./screens/MessagesScreen";
import { PermissionsScreen } from "./screens/PermissionsScreen";
import { AiScreen, AppearanceScreen, MoreScreen } from "./screens/MoreScreen";
import { RoutinesScreen } from "./screens/RoutinesScreen";
import { LifeScreen } from "./screens/LifeScreen";
import { ScreenControlScreen } from "./screens/ScreenControlScreen";
import { SmartHomeScreen } from "./screens/SmartHomeScreen";
import { TodayScreen } from "./screens/TodayScreen";
import { WidgetsScreen } from "./screens/WidgetsScreen";
import { AssistantSheet } from "./AssistantSheet";
import { useSheet } from "./sheet";

restoreTheme();

// IGRIS on screen or not: while it is, "Hey IGRIS" starts listening in the app.
if (typeof document !== "undefined") {
  const report = () =>
    void api
      .phoneOnScreen(document.visibilityState === "visible")
      .catch(() => undefined);
  document.addEventListener("visibilitychange", report);
  report();
}

// Errors on the phone's screen go to IGRIS's log (there is no console to look at on a phone).
if (typeof window !== "undefined") {
  window.addEventListener(
    "error",
    (e) =>
      void api
        .uiEvent("ui_error", String(e.message ?? e))
        .catch(() => undefined),
  );
  window.addEventListener(
    "unhandledrejection",
    (e) =>
      void api.uiEvent("ui_error", String(e.reason)).catch(() => undefined),
  );
}

/** The phone layout: one screen at a time, Home in the middle of everything, the opening animation on top. */
function MobileShell() {
  const ready = useAppLifecycle();
  const settings = useSettingsStore((s) => s.settings);
  useEffect(() => {
    // After the theme effect has applied the setting.
    const t = setTimeout(rememberTheme, 0);
    return () => clearTimeout(t);
  }, [settings.theme]);
  useWidgetActions(ready);
  const sheet = useSheet((s) => s.open);
  return (
    <div className="m-app flex h-full flex-col pt-[env(safe-area-inset-top)] pr-[env(safe-area-inset-right)] pl-[env(safe-area-inset-left)]">
      <main className="relative min-h-0 flex-1 overflow-y-auto">
        <Outlet />
      </main>
      <ReminderAlerts enabled={ready} />
      <DevicePrompts enabled={ready} phone />
      {/* Stays mounted (remounting would replay it); hidden under the panel by CSS. */}
      <Opening />
      {sheet && <AssistantSheet />}
    </div>
  );
}

/** A home-screen widget opened IGRIS asking for something: the orb starts listening. */
function useWidgetActions(ready: boolean) {
  const navigate = useNavigate();
  useEffect(() => {
    if (!ready) return;
    const check = () => {
      if (document.visibilityState !== "visible") return;
      api
        .widgetLaunchAction()
        .then((action) => {
          if (action === "assistant") {
            // "Hey IGRIS" / the assistant gesture: a panel over whatever is on screen, listening.
            useSheet.getState().enter();
            const voice = useVoiceStore.getState();
            if (voice.phase === "idle") void voice.listen();
          } else if (action === "listen") {
            navigate("/");
            const voice = useVoiceStore.getState();
            if (voice.phase !== "listening") void voice.toggle();
          } else if (action === "widgets") {
            navigate("/settings/widgets");
          }
        })
        .catch(() => undefined);
    };
    check();
    document.addEventListener("visibilitychange", check);
    window.addEventListener("focus", check);
    return () => {
      document.removeEventListener("visibilitychange", check);
      window.removeEventListener("focus", check);
    };
  }, [ready, navigate]);
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
      { path: "smart-home", element: <SmartHomeScreen /> },
      { path: "settings", element: <MoreScreen /> },
      { path: "settings/ai", element: <AiScreen /> },
      { path: "settings/appearance", element: <AppearanceScreen /> },
      { path: "settings/memory", element: <MemoryScreen /> },
      { path: "settings/activity", element: <ActivityScreen /> },
      { path: "settings/permissions", element: <PermissionsScreen /> },
      { path: "settings/widgets", element: <WidgetsScreen /> },
      { path: "settings/screen", element: <ScreenControlScreen /> },
      { path: "settings/accounts", element: <AccountsScreen /> },
      { path: "routines", element: <RoutinesScreen /> },
      { path: "life", element: <LifeScreen /> },
      { path: "settings/messages", element: <MessagesScreen /> },
      { path: "settings/devices", element: <DevicesScreen /> },
      { path: "*", element: <Navigate to="/" replace /> },
    ],
  },
]);

/** IGRIS on phones: the same IGRIS (stores, services, backend), its own screens. */
export function MobileApp() {
  return <RouterProvider router={router} />;
}
