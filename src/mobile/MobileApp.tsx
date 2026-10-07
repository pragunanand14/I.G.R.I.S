import "./mobile.css";
import { CalendarCheck, House, LayoutGrid, MessageCircle } from "lucide-react";
import { createHashRouter, Navigate, NavLink, Outlet, RouterProvider } from "react-router";
import { DevicePrompts } from "@/components/devices/DevicePrompts";
import { ReminderAlerts } from "@/components/productivity/ReminderAlerts";
import { useAppLifecycle } from "@/hooks/useAppLifecycle";
import { ActivityScreen } from "./screens/ActivityScreen";
import { ChatScreen } from "./screens/ChatScreen";
import { DevicesScreen } from "./screens/DevicesScreen";
import { HomeScreen } from "./screens/HomeScreen";
import { MemoryScreen } from "./screens/MemoryScreen";
import { MoreScreen } from "./screens/MoreScreen";
import { TodayScreen } from "./screens/TodayScreen";

const TABS = [
  { to: "/", label: "Home", icon: House },
  { to: "/chat", label: "Chat", icon: MessageCircle },
  { to: "/today", label: "Today", icon: CalendarCheck },
  { to: "/more", label: "More", icon: LayoutGrid },
];

/** The phone layout: a screen above a four-tab bar. */
function MobileShell() {
  const ready = useAppLifecycle();
  return (
    <div className="m-app flex h-full flex-col pt-[env(safe-area-inset-top)] pr-[env(safe-area-inset-right)] pl-[env(safe-area-inset-left)]">
      <main className="relative min-h-0 flex-1 overflow-y-auto">
        <Outlet />
      </main>
      <nav aria-label="Main" className="m-tabbar flex shrink-0 pb-[env(safe-area-inset-bottom)]">
        {TABS.map(({ to, label, icon: Icon }) => (
          <NavLink key={to} to={to} end={to === "/"} className="m-tab">
            <span className="m-tab-pill">
              <Icon className="size-[22px]" strokeWidth={1.9} />
            </span>
            {label}
          </NavLink>
        ))}
      </nav>
      <ReminderAlerts enabled={ready} />
      <DevicePrompts enabled={ready} phone />
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
      { path: "today", element: <TodayScreen /> },
      { path: "more", element: <MoreScreen /> },
      { path: "more/memory", element: <MemoryScreen /> },
      { path: "more/activity", element: <ActivityScreen /> },
      { path: "more/devices", element: <DevicesScreen /> },
      { path: "*", element: <Navigate to="/" replace /> },
    ],
  },
]);

/** IGRIS on phones: the same IGRIS (stores, services, backend), its own screens. */
export function MobileApp() {
  return <RouterProvider router={router} />;
}
