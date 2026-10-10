import { openUrl } from "@tauri-apps/plugin-opener";
import { Copy, ExternalLink, Link2, StickyNote, X } from "lucide-react";
import { useState } from "react";
import { hasBackend } from "@/services/backend";
import { useDeviceStore } from "@/stores/deviceStore";

/** Links and notes your other devices sent here. Nothing opens until you tap Open. */
export function HandoffInbox({ phone = false, max = 3 }: { phone?: boolean; max?: number }) {
  const items = useDeviceStore((s) => s.handoffs);
  const dismiss = useDeviceStore((s) => s.dismissHandoff);
  const [copied, setCopied] = useState<string | null>(null);
  if (items.length === 0) return null;

  const copy = (id: string, text: string) =>
    void navigator.clipboard
      ?.writeText(text)
      .then(() => {
        setCopied(id);
        setTimeout(() => setCopied((c) => (c === id ? null : c)), 1500);
      })
      .catch(() => undefined);
  const action = phone ? "m-chip" : "flex items-center gap-1 rounded-full bg-surface-strong px-2.5 py-1 text-[11px] font-medium text-fg hover:bg-surface-hover";

  return (
    <ul className="mx-auto w-full max-w-md space-y-2" aria-label="Sent from your other devices">
      {items.slice(0, max).map((h) => (
        <li
          key={h.id}
          className={`anim-rise flex items-start gap-3 rounded-2xl px-3.5 py-3 text-left ${phone ? "" : "bg-surface"}`}
          style={phone ? { background: "var(--m-surface)" } : undefined}
        >
          <span className="mt-0.5 shrink-0 text-accent" aria-hidden="true">
            {h.kind === "link" ? <Link2 className="size-4" /> : <StickyNote className="size-4" />}
          </span>
          <div className="min-w-0 flex-1">
            <p className={phone ? "m-muted text-[13px]" : "text-[11px] text-faint"}>From {h.fromName}</p>
            <p className={`mt-0.5 line-clamp-3 break-words ${phone ? "text-[15px]" : "text-sm text-fg"}`} data-selectable>
              {h.content}
            </p>
            <div className="mt-2 flex flex-wrap gap-1.5">
              {h.kind === "link" && hasBackend() && /^https?:\/\//i.test(h.content) && (
                <button type="button" className={action} onClick={() => void openUrl(h.content).catch(() => undefined)}>
                  <ExternalLink className="size-3.5" /> Open
                </button>
              )}
              <button type="button" className={action} onClick={() => copy(h.id, h.content)}>
                <Copy className="size-3.5" /> {copied === h.id ? "Copied" : "Copy"}
              </button>
            </div>
          </div>
          <button
            type="button"
            aria-label="Dismiss"
            onClick={() => void dismiss(h.id)}
            className={phone ? "m-icon-button -mt-1 -mr-1 shrink-0" : "shrink-0 rounded-full p-1 text-faint hover:text-fg"}
          >
            <X className="size-4" />
          </button>
        </li>
      ))}
    </ul>
  );
}
