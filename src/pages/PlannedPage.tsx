import { Construction } from "lucide-react";
import { Link } from "react-router";
import type { NavItem } from "@/config/navigation";

/** Honest placeholder for a section that is on the roadmap but not built. */
export function PlannedPage({ item }: { item: NavItem }) {
  const Icon = item.icon;
  return (
    <div className="flex min-h-full items-center justify-center p-8">
      <div className="w-full max-w-lg rounded-[20px] bg-surface p-8">
        <div className="mb-5 flex items-center gap-3">
          <div className="grid size-10 place-items-center rounded-xl bg-surface-strong text-muted">
            <Icon className="size-5" strokeWidth={1.6} />
          </div>
          <div>
            <h1 className="text-lg font-light text-fg">{item.label}</h1>
            <p className="text-xs text-muted">{item.summary}</p>
          </div>
        </div>
        <div className="mb-4 flex items-center gap-2 rounded-lg border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
          <Construction className="size-3.5 shrink-0" />
          Not available yet — planned for Phase {item.plannedPhase}. Nothing on this page is functional.
        </div>
        {item.planned && (
          <>
            <p className="text-label mb-2">Planned scope</p>
            <ul className="space-y-1.5 text-sm text-muted">
              {item.planned.map((p) => (
                <li key={p} className="flex gap-2">
                  <span className="text-faint">—</span>
                  {p}
                </li>
              ))}
            </ul>
          </>
        )}
        <Link to="/" className="mt-6 inline-block text-xs text-accent hover:underline">
          Back to Home
        </Link>
      </div>
    </div>
  );
}
