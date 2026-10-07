import { Check, MessageSquarePlus, Pencil, Trash2, X } from "lucide-react";
import { useState } from "react";
import type { Conversation } from "@/types/chat";

function relative(iso: string): string {
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return "";
  const diff = Date.now() - t;
  const min = Math.round(diff / 60000);
  if (min < 1) return "now";
  if (min < 60) return `${min}m`;
  const h = Math.round(min / 60);
  if (h < 24) return `${h}h`;
  const d = Math.round(h / 24);
  if (d < 7) return `${d}d`;
  return new Date(iso).toLocaleDateString([], { month: "short", day: "numeric" });
}

interface Props {
  conversations: Conversation[];
  activeId: string | null;
  busyId: string | null;
  onSelect: (id: string | null) => void;
  onRename: (id: string, title: string) => Promise<boolean>;
  onDelete: (id: string) => Promise<boolean>;
  /** Narrow screens: the list is a panel over the chat, shown on request. Wide screens always show it. */
  openOnNarrow?: boolean;
}

export function ConversationList({ conversations, activeId, busyId, onSelect, onRename, onDelete, openOnNarrow = false }: Props) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [confirmId, setConfirmId] = useState<string | null>(null);

  return (
    <aside
      className={`${openOnNarrow ? "absolute inset-0 z-20 flex w-full bg-bg" : "hidden"} shrink-0 flex-col border-r border-line md:static md:z-auto md:flex md:w-64 md:bg-bg/60`}
    >
      <div className="p-3">
        <button
          type="button"
          onClick={() => onSelect(null)}
          className="flex w-full items-center gap-2 rounded-lg border border-line bg-surface px-3 py-2 text-sm text-fg transition-colors hover:bg-surface-hover"
        >
          <MessageSquarePlus className="size-4 text-accent" />
          New conversation
        </button>
      </div>
      <p className="text-label px-4 pb-2">History</p>
      <ul className="flex-1 space-y-0.5 overflow-y-auto px-2 pb-3" aria-label="Conversations">
        {conversations.length === 0 && <li className="px-3 py-2 text-xs text-faint">No conversations yet.</li>}
        {conversations.map((c) => {
          const active = c.id === activeId;
          if (editingId === c.id) {
            return (
              <li key={c.id}>
                <form
                  className="flex items-center gap-1 rounded-lg bg-surface-strong px-2 py-1.5"
                  onSubmit={async (e) => {
                    e.preventDefault();
                    if (await onRename(c.id, draft)) setEditingId(null);
                  }}
                >
                  <input
                    value={draft}
                    autoFocus
                    maxLength={80}
                    onChange={(e) => setDraft(e.target.value)}
                    onKeyDown={(e) => e.key === "Escape" && setEditingId(null)}
                    aria-label="Conversation title"
                    className="min-w-0 flex-1 bg-transparent text-sm text-fg focus:outline-none"
                  />
                  <button type="submit" aria-label="Save title" className="text-muted hover:text-fg">
                    <Check className="size-3.5" />
                  </button>
                  <button type="button" aria-label="Cancel rename" onClick={() => setEditingId(null)} className="text-muted hover:text-fg">
                    <X className="size-3.5" />
                  </button>
                </form>
              </li>
            );
          }
          return (
            <li key={c.id} className="group relative">
              <button
                type="button"
                onClick={() => onSelect(c.id)}
                aria-current={active ? "page" : undefined}
                className={`flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm transition-colors ${
                  active ? "bg-surface-strong text-fg" : "text-muted hover:bg-surface-hover hover:text-fg"
                }`}
              >
                <span className="min-w-0 flex-1 truncate">{c.title}</span>
                {busyId === c.id ? (
                  <span className="size-1.5 shrink-0 animate-pulse rounded-full bg-accent" aria-label="Responding" />
                ) : (
                  <span className="shrink-0 text-[10px] text-faint group-hover:invisible">{relative(c.updatedAt)}</span>
                )}
              </button>
              <div className="absolute top-1/2 right-1.5 hidden -translate-y-1/2 items-center gap-0.5 rounded-md bg-elevated group-hover:flex">
                {confirmId === c.id ? (
                  <button
                    type="button"
                    onClick={async () => {
                      await onDelete(c.id);
                      setConfirmId(null);
                    }}
                    onMouseLeave={() => setConfirmId(null)}
                    className="rounded px-1.5 py-0.5 text-[11px] font-medium text-danger hover:bg-danger/10"
                  >
                    Delete?
                  </button>
                ) : (
                  <>
                    <button
                      type="button"
                      aria-label={`Rename ${c.title}`}
                      onClick={() => (setDraft(c.title), setEditingId(c.id))}
                      className="grid size-6 place-items-center rounded text-faint hover:text-fg"
                    >
                      <Pencil className="size-3" />
                    </button>
                    <button
                      type="button"
                      aria-label={`Delete ${c.title}`}
                      disabled={busyId === c.id}
                      onClick={() => setConfirmId(c.id)}
                      className="grid size-6 place-items-center rounded text-faint hover:text-danger disabled:opacity-30"
                    >
                      <Trash2 className="size-3" />
                    </button>
                  </>
                )}
              </div>
            </li>
          );
        })}
      </ul>
    </aside>
  );
}
