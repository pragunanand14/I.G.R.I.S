import { createHashRouter, RouterProvider } from "react-router";
import { NAV_ITEMS } from "@/config/navigation";
import { AppShell } from "@/layouts/AppShell";
import { lazy, Suspense } from "react";
import { isMobilePlatform } from "@/services/platform";
import { HomePage } from "@/pages/HomePage";
import { NotFoundPage } from "@/pages/NotFoundPage";
import { PlannedPage } from "@/pages/PlannedPage";

// Pages load on first visit; Home (the start screen) ships in the main bundle.
const named = <K extends string>(load: () => Promise<Record<K, React.ComponentType>>, key: K) =>
  lazy(() => load().then((m) => ({ default: m[key] })));
const ChatPage = named(() => import("@/pages/ChatPage"), "ChatPage");
const MemoryPage = named(() => import("@/pages/MemoryPage"), "MemoryPage");
const ProjectsPage = named(() => import("@/pages/ProjectsPage"), "ProjectsPage");
const SecurityPage = named(() => import("@/pages/SecurityPage"), "SecurityPage");
const SettingsPage = named(() => import("@/pages/SettingsPage"), "SettingsPage");
const SystemPage = named(() => import("@/pages/SystemPage"), "SystemPage");
const TasksPage = named(() => import("@/pages/TasksPage"), "TasksPage");
const ToolsPage = named(() => import("@/pages/ToolsPage"), "ToolsPage");

function Page({ children }: { children: React.ReactNode }) {
  return <Suspense fallback={<div className="p-8 text-sm text-faint">Loading…</div>}>{children}</Suspense>;
}

const IMPLEMENTED: Record<string, React.ReactNode> = {
  "/chat": <ChatPage />,
  "/tasks": <TasksPage />,
  "/memory": <MemoryPage />,
  "/system": <SystemPage />,
  "/tools": <ToolsPage />,
  "/projects": <ProjectsPage />,
  "/security": <SecurityPage />,
  "/settings": <SettingsPage />,
};

// Hash routing: the app is served from a custom protocol in production.
const router = createHashRouter([
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <HomePage /> },
      ...NAV_ITEMS.filter((n) => n.path !== "/").map((item) => ({
        path: item.path.slice(1),
        element: IMPLEMENTED[item.path] ? <Page>{IMPLEMENTED[item.path]}</Page> : <PlannedPage item={item} />,
      })),
      { path: "*", element: <NotFoundPage /> },
    ],
  },
]);

// Phones get their own UI (same IGRIS underneath: stores, services, backend).
const MobileApp = named(() => import("@/mobile/MobileApp"), "MobileApp");

export function App() {
  if (isMobilePlatform()) {
    return (
      <Suspense fallback={null}>
        <MobileApp />
      </Suspense>
    );
  }
  return <RouterProvider router={router} />;
}
