import { X } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import type { AllowedAction } from "@/types/tools";

/**
 * Actions the user chose to "Always allow", each removable. Phone and desktop
 * share it; `phone` switches to the phone's row style.
 */
export function AllowedActions({ phone = false }: { phone?: boolean }) {
  const [items, setItems] = useState<AllowedAction[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState(0);

  useEffect(() => {
    let live = true;
    api
      .listAllowedActions()
      .then((r) => {
        if (!live) return;
        setItems(r);
        setError(null);
      })
      .catch((err) => live && setError(BackendError.from(err).message));
    return () => {
      live = false;
    };
  }, [version]);

  const act = (p: Promise<unknown>) =>
    void p.then(
      () => setVersion((v) => v + 1),
      (err) => setError(BackendError.from(err).message),
    );

  if (error) return <p className={`text-sm text-danger ${phone ? "px-1" : ""}`}>{error}</p>;
  if (!items) return null;
  if (items.length === 0)
    return (
      <p className={`text-sm ${phone ? "m-muted px-1" : "text-faint"}`}>
        Nothing yet. When IGRIS asks before doing something, tap <b>Always allow</b> and it won't ask for that again.
      </p>
    );

  return (
    <div className="space-y-2">
      <ul className={phone ? "m-group" : "divide-y divide-line"} aria-label="Always allowed">
        {items.map((a) => (
          <li key={a.id} className={phone ? "m-row" : "flex items-center gap-3 py-2.5 first:pt-0 last:pb-0"}>
            <span className={`min-w-0 flex-1 break-words ${phone ? "text-[15px]" : "text-sm text-fg"}`}>{a.label}</span>
            <button
              type="button"
              aria-label={`Stop always allowing: ${a.label}`}
              title="Ask me again next time"
              onClick={() => act(api.removeAllowedAction(a.id))}
              className={phone ? "m-icon-button shrink-0" : "shrink-0 rounded-full p-1.5 text-muted hover:bg-surface-hover hover:text-fg"}
            >
              <X className="size-4" />
            </button>
          </li>
        ))}
      </ul>
      {items.length > 1 && (
        <button
          type="button"
          onClick={() => act(api.clearAllowedActions())}
          className={phone ? "m-button w-full" : "rounded-full bg-surface-strong px-3.5 py-1.5 text-xs font-medium text-fg hover:bg-surface-hover"}
        >
          Ask me again for everything
        </button>
      )}
    </div>
  );
}
