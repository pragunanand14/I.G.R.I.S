import { createHashRouter, RouterProvider } from "react-router";
import { NAV_ITEMS } from "@/config/navigation";
import { AppShell } from "@/layouts/AppShell";
import { ChatPage } from "@/pages/ChatPage";
import { HomePage } from "@/pages/HomePage";
import { NotFoundPage } from "@/pages/NotFoundPage";
import { PlannedPage } from "@/pages/PlannedPage";
import { SettingsPage } from "@/pages/SettingsPage";
import { SystemPage } from "@/pages/SystemPage";

const IMPLEMENTED: Record<string, React.ReactNode> = {
  "/chat": <ChatPage />,
  "/system": <SystemPage />,
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
        element: IMPLEMENTED[item.path] ?? <PlannedPage item={item} />,
      })),
      { path: "*", element: <NotFoundPage /> },
    ],
  },
]);

export function App() {
  return <RouterProvider router={router} />;
}
