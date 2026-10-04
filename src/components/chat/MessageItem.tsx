import { AlertTriangle, Ban, Check, Copy, Pencil, RotateCcw, Scissors, ShieldAlert } from "lucide-react";
import { memo, useState, type ReactNode } from "react";
import type { Message } from "@/types/chat";
import { formatTime } from "@/utils/format";
import { Markdown } from "./Markdown";


function ActionButton({ label, onClick, disabled, children }: { label: string; onClick: () => void; disabled?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className="grid size-7 place-items-center rounded-md text-faint transition-colors hover:bg-surface-hover hover:text-fg disabled:pointer-events-none disabled:opacity-40"
    >
      {children}
    </button>
  );
}

function StatusNote({ message, onRetry, canRetry }: { message: Message; onRetry?: () => void; canRetry: boolean }) {
  const map = {
    error: { icon: <AlertTriangle className="size-3.5" />, cls: "border-danger/30 bg-danger/10 text-danger", text: message.error ?? "Something went wrong." },
    refused: { icon: <ShieldAlert className="size-3.5" />, cls: "border-warning/30 bg-warning/10 text-warning", text: message.error ?? "The model declined this request." },
    cancelled: { icon: <Ban className="size-3.5" />, cls: "border-line bg-surface text-muted", text: "Stopped." },
    truncated: { icon: <Scissors className="size-3.5" />, cls: "border-line bg-surface text-muted", text: message.error ?? "Cut off at the length limit." },
  } as const;
  if (message.status === "complete") return null;
  const s = map[message.status];
  return (
    <div className={`mt-2 flex items-center gap-2 rounded-lg border px-3 py-1.5 text-xs ${s.cls}`} role={message.status === "error" ? "alert" : undefined}>
      {s.icon}
      <span className="min-w-0 flex-1">{s.text}</span>
      {canRetry && onRetry && (
        <button type="button" onClick={onRetry} className="shrink-0 font-medium underline-offset-2 hover:underline">
          Retry
        </button>
      )}
    </div>
  );
}

interface Props {
  message: Message;
  isLastAssistant: boolean;
  busy: boolean;
  onRegenerate: () => void;
  onEdit: (id: string, content: string) => Promise<boolean>;
}

export const MessageItem = memo(function MessageItem({ message, isLastAssistant, busy, onRegenerate, onEdit }: Props) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(message.content);
  const [copied, setCopied] = useState(false);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(message.content);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  };

  if (message.role === "user") {
    return (
      <div className="group flex flex-col items-end gap-1">
        {editing ? (
          <form
            className="w-full max-w-[80%]"
            onSubmit={async (e) => {
              e.preventDefault();
              if (!draft.trim()) return;
              setEditing(false);
              await onEdit(message.id, draft.trim());
            }}
          >
            <textarea
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              autoFocus
              rows={Math.min(10, draft.split("\n").length + 1)}
              aria-label="Edit message"
              className="w-full resize-y rounded-xl border border-accent bg-elevated px-4 py-2.5 text-sm text-fg focus:outline-none"
            />
            <div className="mt-1.5 flex justify-end gap-2 text-xs">
              <button type="button" onClick={() => (setEditing(false), setDraft(message.content))} className="rounded-md px-2.5 py-1 text-muted hover:bg-surface-hover">
                Cancel
              </button>
              <button type="submit" disabled={busy || !draft.trim()} className="rounded-md bg-accent px-2.5 py-1 font-medium text-bg disabled:opacity-40">
                Save &amp; resend
              </button>
            </div>
            <p className="mt-1 text-right text-[11px] text-faint">Messages after this one will be replaced.</p>
          </form>
        ) : (
          <div className="max-w-[80%] rounded-2xl rounded-br-md bg-surface-strong px-4 py-2.5 text-sm whitespace-pre-wrap text-fg" data-selectable>
            {message.content}
          </div>
        )}
        {!editing && (
          <div className="flex items-center gap-0.5 opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100">
            <span className="mr-1 text-[10px] text-faint">{formatTime(message.createdAt)}</span>
            <ActionButton label={copied ? "Copied" : "Copy"} onClick={() => void copy()}>
              {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
            </ActionButton>
            <ActionButton label="Edit" disabled={busy} onClick={() => (setDraft(message.content), setEditing(true))}>
              <Pencil className="size-3.5" />
            </ActionButton>
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="group flex gap-3">
      <div className="mt-1 size-6 shrink-0 rounded-full border border-line bg-surface p-[5px]" aria-hidden="true">
        <div className="size-full rounded-full bg-accent" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="mb-1 flex items-baseline gap-2">
          <span className="text-xs font-semibold tracking-[0.18em] text-fg">IGRIS</span>
          <span className="text-[10px] text-faint">{formatTime(message.createdAt)}</span>
        </div>
        {message.content && <Markdown text={message.content} />}
        <StatusNote message={message} onRetry={onRegenerate} canRetry={isLastAssistant && !busy && message.status !== "refused"} />
        <div className="mt-1 flex items-center gap-0.5 opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100">
          {message.content && (
            <ActionButton label={copied ? "Copied" : "Copy"} onClick={() => void copy()}>
              {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
            </ActionButton>
          )}
          {isLastAssistant && (
            <ActionButton label="Regenerate" disabled={busy} onClick={onRegenerate}>
              <RotateCcw className="size-3.5" />
            </ActionButton>
          )}
          {message.model && <span className="ml-1 font-mono text-[10px] text-faint">{message.model}</span>}
        </div>
      </div>
    </div>
  );
});
