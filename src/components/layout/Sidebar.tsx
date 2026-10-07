import { NavLink } from "react-router";
import { NAV_ITEMS } from "@/config/navigation";
import { useAppStore } from "@/stores/appStore";
import { StatusDot } from "@/components/ui/StatusDot";
import { DESKTOP_ONLY_PATHS, isMobilePlatform } from "@/services/platform";

export function Sidebar() {
  const backend = useAppStore((s) => s.backend);
  const tone = backend === "ready" ? "ok" : backend === "connecting" ? "warn" : "error";
  const backendLabel =
    backend === "ready" ? "Backend connected" : backend === "connecting" ? "Connecting to backend" : "Backend unavailable";

  const items = isMobilePlatform() ? NAV_ITEMS.filter((i) => !DESKTOP_ONLY_PATHS.includes(i.path)) : NAV_ITEMS;

  return (
    <nav
      aria-label="Primary"
      className="flex w-full shrink-0 flex-row items-center border-t border-line bg-bg px-1 py-1 md:w-[72px] md:flex-col md:border-t-0 md:border-r md:px-0 md:py-3"
    >
      <ul className="flex min-w-0 flex-1 flex-row items-center gap-1 overflow-x-auto md:flex-col md:overflow-visible">
        {items.map((item) => {
          const Icon = item.icon;
          const planned = item.plannedPhase !== undefined;
          return (
            <li key={item.path}>
              <NavLink
                to={item.path}
                end={item.path === "/"}
                title={planned ? `${item.label} — planned for Phase ${item.plannedPhase}` : item.label}
                className={({ isActive }) =>
                  `group relative flex w-14 flex-col items-center gap-1 rounded-lg py-2 transition-colors ${
                    isActive ? "bg-surface-strong text-fg" : "text-muted hover:bg-surface-hover hover:text-fg"
                  } ${planned ? "opacity-45" : ""}`
                }
              >
                {({ isActive }) => (
                  <>
                    {isActive && (
                      <span className="absolute -top-1 right-3 left-3 h-0.5 rounded-full bg-accent md:top-2 md:right-auto md:bottom-2 md:-left-2 md:h-auto md:w-0.5" />
                    )}
                    <Icon className="size-[18px]" strokeWidth={1.6} />
                    <span className="text-[10px] leading-none">{item.label}</span>
                    {planned && (
                      <span className="absolute top-1 right-1 rounded px-0.5 font-mono text-[8px] leading-tight text-faint">
                        P{item.plannedPhase}
                      </span>
                    )}
                  </>
                )}
              </NavLink>
            </li>
          );
        })}
      </ul>
      <div className="hidden flex-col items-center gap-1 pt-2 md:flex" title={backendLabel} aria-label={backendLabel} role="status">
        <StatusDot tone={tone} pulse={backend === "connecting"} />
        <span className="text-[9px] tracking-wider text-faint uppercase">Core</span>
      </div>
    </nav>
  );
}
