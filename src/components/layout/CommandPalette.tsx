import { CornerDownLeft, Moon, SquarePen, Sun } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useNavigate } from "react-router";
import { NAV_ITEMS } from "@/config/navigation";
import { nav } from "@/hooks/motion";
import { useChatStore } from "@/stores/chatStore";
import { useSettingsStore } from "@/stores/settingsStore";

interface Command {
  id: string;
  label: string;
  hint: string;
  icon: ReactNode;
  run: () => void;
}

/** Ctrl+K: jump to any page, start a new chat, switch light/dark. */
export function CommandPalette({ open, onClose }: { open: boolean; onClose: () => void }) {
  return open ? <Palette onClose={onClose} /> : null;
}

function Palette({ onClose }: { onClose: () => void }) {
  const navigate = useNavigate();
  const theme = useSettingsStore((s) => s.settings.theme);
  const update = useSettingsStore((s) => s.update);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const input = useRef<HTMLInputElement>(null);

  const commands = useMemo<Command[]>(() => {
    const go = (path: string) => () => void navigate(path, nav());
    const light = document.documentElement.dataset.theme === "light";
    return [
      {
        id: "new-chat",
        label: "New chat",
        hint: "Start a conversation",
        icon: <SquarePen className="size-4" />,
        run: () => {
          void useChatStore.getState().openConversation(null);
          void navigate("/chat", nav());
        },
      },
      ...NAV_ITEMS.map((i) => {
        const Icon = i.icon;
        return { id: i.path, label: i.label, hint: i.summary, icon: <Icon className="size-4" />, run: go(i.path) };
      }),
      {
        id: "theme",
        label: light ? "Dark theme" : "Light theme",
        hint: theme === "system" ? "Stops following the system" : "Switch the look",
        icon: light ? <Moon className="size-4" /> : <Sun className="size-4" />,
        run: () => void update({ theme: light ? "dark" : "light" }),
      },
    ];
  }, [navigate, theme, update]);

  const q = query.trim().toLowerCase();
  const shown = q ? commands.filter((c) => c.label.toLowerCase().includes(q) || c.hint.toLowerCase().includes(q)) : commands;

  useEffect(() => input.current?.focus(), []);

  const choose = (c: Command | undefined) => {
    if (!c) return;
    onClose();
    c.run();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center pt-[16vh]" role="dialog" aria-modal="true" aria-label="Go to">
      <div className="anim-fade absolute inset-0 bg-black/40 backdrop-blur-[2px]" onClick={onClose} aria-hidden="true" />
      <div className="palette-in relative w-full max-w-lg overflow-hidden rounded-2xl border border-line-strong bg-elevated shadow-[var(--shadow)]">
        <input
          ref={input}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setIndex(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "Escape") onClose();
            else if (e.key === "ArrowDown") {
              e.preventDefault();
              setIndex((i) => Math.min(shown.length - 1, i + 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setIndex((i) => Math.max(0, i - 1));
            } else if (e.key === "Enter") {
              e.preventDefault();
              choose(shown[index]);
            }
          }}
          placeholder="Go to…"
          aria-label="Go to"
          className="h-14 w-full border-b border-line bg-transparent px-5 text-[15px] text-fg placeholder:text-faint focus:outline-none"
        />
        <ul className="max-h-80 overflow-y-auto p-2" role="listbox" aria-label="Commands">
          {shown.length === 0 && <li className="px-3 py-6 text-center text-sm text-faint">Nothing matches.</li>}
          {shown.map((c, i) => (
            <li key={c.id} role="option" aria-selected={i === index}>
              <button
                type="button"
                onMouseEnter={() => setIndex(i)}
                onClick={() => choose(c)}
                className={`flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left ${i === index ? "bg-surface-strong text-fg" : "text-muted"}`}
              >
                <span className="shrink-0">{c.icon}</span>
                <span className="min-w-0 flex-1">
                  <span className="block text-sm font-medium text-fg">{c.label}</span>
                  <span className="block truncate text-xs text-faint">{c.hint}</span>
                </span>
                {i === index && <CornerDownLeft className="size-3.5 shrink-0 text-faint" />}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
