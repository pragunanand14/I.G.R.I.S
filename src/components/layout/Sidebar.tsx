import { useLayoutEffect, useRef, useState } from "react";
import { NavLink, useLocation } from "react-router";
import { AiCore } from "@/components/core/AiCore";
import { NAV_ITEMS, type NavItem } from "@/config/navigation";
import { useCoreState } from "@/hooks/useCoreState";
import { nav } from "@/hooks/motion";
import { useAppStore } from "@/stores/appStore";
import { StatusDot } from "@/components/ui/StatusDot";

const MAIN = NAV_ITEMS.filter((i) => i.path !== "/settings");
const SETTINGS = NAV_ITEMS.find((i) => i.path === "/settings");

/**
 * The window's left column: the IGRIS mark (also a drag handle), the pages,
 * Settings and the engine's state. The highlight slides to the current page.
 */
export function Sidebar() {
  const backend = useAppStore((s) => s.backend);
  const version = useAppStore((s) => s.info?.version);
  const core = useCoreState();
  const { pathname } = useLocation();
  const list = useRef<HTMLDivElement>(null);
  const [mark, setMark] = useState<{ y: number; h: number } | null>(null);

  const current = NAV_ITEMS.find((i) => (i.path === "/" ? pathname === "/" : pathname.startsWith(i.path)))?.path ?? null;
  useLayoutEffect(() => {
    const el = current ? list.current?.querySelector<HTMLElement>(`[data-path="${current}"]`) : null;
    setMark(el ? { y: el.offsetTop, h: el.offsetHeight } : null);
  }, [current]);

  const tone = backend === "ready" ? "ok" : backend === "connecting" ? "warn" : "error";
  const status = backend === "ready" ? "Online" : backend === "connecting" ? "Starting" : "Offline";

  return (
    <nav aria-label="Primary" className="flex w-16 shrink-0 flex-col pb-3 lg:w-56">
      <div data-tauri-drag-region className="flex h-14 shrink-0 items-center gap-2.5 px-5">
        <span className="size-6 shrink-0" aria-hidden="true">
          <AiCore state={core} size="100%" />
        </span>
        <span data-tauri-drag-region className="hidden text-[13px] font-semibold tracking-[0.28em] text-fg lg:inline">
          IGRIS
        </span>
      </div>

      <div ref={list} className="relative mt-2 flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto px-3">
        {mark && (
          <span
            aria-hidden="true"
            className="pointer-events-none absolute right-3 left-3 rounded-lg bg-surface-strong transition-[transform,height] duration-300 ease-[var(--ease-out)]"
            style={{ transform: `translateY(${mark.y}px)`, height: mark.h, top: 0 }}
          />
        )}
        {MAIN.map((item) => (
          <Item key={item.path} item={item} />
        ))}
        <div className="flex-1" />
        {SETTINGS && <Item item={SETTINGS} />}
      </div>

      <div className="mt-3 flex items-center gap-2 px-5 text-xs text-faint" role="status" title={`IGRIS core: ${status}`}>
        <StatusDot tone={tone} pulse={backend === "connecting"} />
        <span className="hidden lg:inline">{status}</span>
        {version && <span className="ml-auto hidden font-mono text-[10px] lg:inline">v{version}</span>}
      </div>
    </nav>
  );
}

function Item({ item }: { item: NavItem }) {
  const Icon = item.icon;
  const planned = item.plannedPhase !== undefined;
  return (
    <NavLink
      to={item.path}
      end={item.path === "/"}
      data-path={item.path}
      viewTransition={nav().viewTransition}
      title={planned ? `${item.label} — planned for Phase ${item.plannedPhase}` : item.label}
      className={({ isActive }) =>
        `relative flex h-9 items-center justify-center gap-3 rounded-lg px-3 text-[13px] font-medium transition-colors duration-200 lg:justify-start ${
          isActive ? "text-fg" : "text-muted hover:bg-surface-hover hover:text-fg"
        } ${planned ? "opacity-45" : ""}`
      }
    >
      <Icon className="size-[18px] shrink-0" strokeWidth={1.7} />
      <span className="hidden truncate lg:inline">{item.label}</span>
    </NavLink>
  );
}
