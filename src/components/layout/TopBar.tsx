import { Search, Settings } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import { NavLink, useLocation } from "react-router";
import { AiCore } from "@/components/core/AiCore";
import { StatusDot } from "@/components/ui/StatusDot";
import { NAV_ITEMS } from "@/config/navigation";
import { nav } from "@/hooks/motion";
import { useCoreState } from "@/hooks/useCoreState";
import { useAppStore } from "@/stores/appStore";
import { WindowControls } from "./WindowControls";

const PAGES = NAV_ITEMS.filter((i) => i.path !== "/settings");

/**
 * The one bar at the top of the window: the IGRIS mark and state on the left,
 * the pages in a floating pill in the middle (the highlight glides to the
 * current one), quick switch, Settings and the window buttons on the right.
 * The empty parts drag the window.
 */
export function TopBar({ onCommand }: { onCommand: () => void }) {
  const backend = useAppStore((s) => s.backend);
  const core = useCoreState();
  const { pathname } = useLocation();
  const pill = useRef<HTMLDivElement>(null);
  const [thumb, setThumb] = useState<{ x: number; w: number } | null>(null);

  const current = PAGES.find((i) => (i.path === "/" ? pathname === "/" : pathname.startsWith(i.path)))?.path ?? null;
  useLayoutEffect(() => {
    const measure = () => {
      const el = current ? pill.current?.querySelector<HTMLElement>(`[data-path="${current}"]`) : null;
      setThumb(el ? { x: el.offsetLeft, w: el.offsetWidth } : null);
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [current]);

  const tone = backend === "ready" ? "ok" : backend === "connecting" ? "warn" : "error";
  const status = backend === "ready" ? "Online" : backend === "connecting" ? "Starting" : "Offline";

  return (
    <header data-tauri-drag-region className="relative z-30 grid h-16 shrink-0 grid-cols-[1fr_auto_1fr] items-center gap-4 px-5">
      <div data-tauri-drag-region className="flex min-w-0 items-center gap-2.5" role="status" title={`IGRIS core: ${status}`}>
        <span className="size-6 shrink-0" aria-hidden="true">
          <AiCore state={core} size="100%" />
        </span>
        <span data-tauri-drag-region className="text-[13px] font-semibold tracking-[0.28em] text-fg">
          IGRIS
        </span>
        <StatusDot tone={tone} pulse={backend === "connecting"} />
      </div>

      <nav aria-label="Primary">
        <div ref={pill} className="relative flex items-center gap-0.5 rounded-full bg-surface-strong p-1">
          {thumb && (
            <span
              aria-hidden="true"
              className="absolute top-1 bottom-1 left-0 rounded-full bg-elevated shadow-sm transition-[transform,width] duration-[420ms] ease-[var(--ease-out)]"
              style={{ transform: `translateX(${thumb.x}px)`, width: thumb.w }}
            />
          )}
          {PAGES.map((item) => {
            const Icon = item.icon;
            return (
              <NavLink
                key={item.path}
                to={item.path}
                end={item.path === "/"}
                data-path={item.path}
                viewTransition={nav().viewTransition}
                title={item.label}
                className={({ isActive }) =>
                  `relative flex h-8 items-center gap-2 rounded-full px-3 text-[13px] font-medium transition-colors duration-200 xl:px-3.5 ${
                    isActive ? "text-fg" : "text-muted hover:text-fg"
                  }`
                }
              >
                <Icon className="size-4 xl:hidden" strokeWidth={1.8} aria-hidden="true" />
                <span className="sr-only xl:not-sr-only">{item.label}</span>
              </NavLink>
            );
          })}
        </div>
      </nav>

      <div data-tauri-drag-region className="flex items-center justify-end gap-1">
        <button
          type="button"
          onClick={onCommand}
          className="flex h-8 items-center gap-2 rounded-full px-3 text-[13px] text-muted hover:bg-surface-hover hover:text-fg"
          aria-label="Go to… (Ctrl+K)"
          title="Go to… (Ctrl+K)"
        >
          <Search className="size-4" />
          <kbd className="hidden rounded-md bg-surface-strong px-1.5 py-0.5 font-sans text-[10px] text-faint lg:inline">Ctrl K</kbd>
        </button>
        <NavLink
          to="/settings"
          viewTransition={nav().viewTransition}
          aria-label="Settings"
          title="Settings"
          className={({ isActive }) => `grid size-8 place-items-center rounded-full transition-colors duration-200 ${isActive ? "bg-surface-strong text-fg" : "text-muted hover:bg-surface-hover hover:text-fg"}`}
        >
          <Settings className="size-[18px]" strokeWidth={1.8} />
        </NavLink>
        <WindowControls />
      </div>
    </header>
  );
}
