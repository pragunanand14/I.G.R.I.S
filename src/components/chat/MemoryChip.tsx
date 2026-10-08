import { Brain, ChevronDown } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";
import type { MemoryContext } from "@/types/memory";
import { MEMORY_KIND_LABEL } from "@/types/memory";

/** Shows which saved memories were sent along with a message. */
export function MemoryChip({ context }: { context: MemoryContext }) {
  const [open, setOpen] = useState(false);
  const n = context.items.length;
  return (
    <div className="flex max-w-[80%] flex-col items-end">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className="flex items-center gap-1 rounded-full bg-surface-strong px-2 py-0.5 text-[10px] text-faint hover:text-muted"
      >
        <Brain className="size-3" />
        {n} {n === 1 ? "memory" : "memories"} used
        <ChevronDown className={`size-3 transition-transform ${open ? "rotate-180" : ""}`} />
      </button>
      {open && (
        <ul className="mt-1 w-full space-y-1 rounded-lg border border-line bg-surface p-2 text-left text-xs" data-selectable>
          {context.items.map((m) => (
            <li key={`${m.id}-${m.updatedAt}`} className="flex gap-2">
              <span className="shrink-0 font-mono text-[10px] text-faint">#{m.id}</span>
              <span className="min-w-0 flex-1 text-muted">{m.content}</span>
              <span className="shrink-0 text-[10px] text-faint">{MEMORY_KIND_LABEL[m.kind]}</span>
            </li>
          ))}
          <li className="pt-1 text-right">
            <Link to="/memory" className="text-[10px] text-accent hover:underline">
              Manage memory
            </Link>
          </li>
        </ul>
      )}
    </div>
  );
}
