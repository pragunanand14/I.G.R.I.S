import { NavLink } from "react-router";
import { NAV_ITEMS } from "@/config/navigation";
import { useAppStore } from "@/stores/appStore";
import { StatusDot } from "@/components/ui/StatusDot";

export function Sidebar() {
  const backend = useAppStore((s) => s.backend);
  const tone = backend === "ready" ? "ok" : backend === "connecting" ? "warn" : "error";
  const backendLabel =
    backend === "ready" ? "Backend connected" : backend === "connecting" ? "Connecting to backend" : "Backend unavailable";

  return (
    <nav aria-label="Primary" className="flex w-[72px] shrink-0 flex-col items-center border-r border-line bg-bg py-3">
      <ul className="flex flex-1 flex-col items-center gap-1">
        {NAV_ITEMS.map((item) => {
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
                    {isActive && <span className="absolute top-2 bottom-2 -left-2 w-0.5 rounded-full bg-accent" />}
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
      <div className="flex flex-col items-center gap-1 pt-2" title={backendLabel} aria-label={backendLabel} role="status">
        <StatusDot tone={tone} pulse={backend === "connecting"} />
        <span className="text-[9px] tracking-wider text-faint uppercase">Core</span>
      </div>
    </nav>
  );
}
