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
const PAGES = {
  chat: () => import("@/pages/ChatPage"),
  memory: () => import("@/pages/MemoryPage"),
  projects: () => import("@/pages/ProjectsPage"),
  security: () => import("@/pages/SecurityPage"),
  settings: () => import("@/pages/SettingsPage"),
  system: () => import("@/pages/SystemPage"),
  tasks: () => import("@/pages/TasksPage"),
  tools: () => import("@/pages/ToolsPage"),
};
const ChatPage = named(PAGES.chat, "ChatPage");
const MemoryPage = named(PAGES.memory, "MemoryPage");
const ProjectsPage = named(PAGES.projects, "ProjectsPage");
const SecurityPage = named(PAGES.security, "SecurityPage");
const SettingsPage = named(PAGES.settings, "SettingsPage");
const SystemPage = named(PAGES.system, "SystemPage");
const TasksPage = named(PAGES.tasks, "TasksPage");
const ToolsPage = named(PAGES.tools, "ToolsPage");

// Once the window is up, fetch the other pages in the background so moving between them never waits.
if (typeof window !== "undefined" && !isMobilePlatform()) {
  setTimeout(() => Object.values(PAGES).forEach((load) => void load().catch(() => undefined)), 2500);
}

function Page({ children }: { children: React.ReactNode }) {
  return <Suspense fallback={null}>{children}</Suspense>;
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
