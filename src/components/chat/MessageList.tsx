import { useEffect, useRef } from "react";
import type { StreamingState } from "@/stores/chatStore";
import type { Message } from "@/types/chat";
import { Markdown } from "./Markdown";
import { MessageItem } from "./MessageItem";

interface Props {
  messages: Message[];
  pendingUser: string | null;
  streaming: StreamingState | null;
  onRegenerate: () => void;
  onEdit: (id: string, content: string) => Promise<boolean>;
}

export function MessageList({ messages, pendingUser, streaming, onRegenerate, onEdit }: Props) {
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const busy = streaming !== null;
  const lastAssistantId = [...messages].reverse().find((m) => m.role === "assistant")?.id;
  const lastIsUser = messages.at(-1)?.role === "user";

  // Follow the stream while the user is at the bottom; stop if they scroll up.
  useEffect(() => {
    const el = scroller.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [messages, pendingUser, streaming?.text]);

  return (
    <div
      ref={scroller}
      onScroll={(e) => {
        const el = e.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
      }}
      className="flex-1 overflow-y-auto"
    >
      <div className="mx-auto flex max-w-3xl flex-col gap-6 px-6 py-8" role="log" aria-live="polite" aria-busy={busy}>
        {messages.map((m) => (
          <MessageItem
            key={m.id}
            message={m}
            isLastAssistant={m.id === lastAssistantId && !lastIsUser}
            busy={busy}
            onRegenerate={onRegenerate}
            onEdit={onEdit}
          />
        ))}
        {pendingUser && (
          <div className="flex justify-end">
            <div className="max-w-[80%] rounded-2xl rounded-br-md bg-surface-strong px-4 py-2.5 text-sm whitespace-pre-wrap text-fg opacity-70">
              {pendingUser}
            </div>
          </div>
        )}
        {streaming && (
          <div className="flex gap-3">
            <div className="mt-1 size-6 shrink-0 rounded-full border border-line bg-surface p-[5px]" aria-hidden="true">
              <div className="size-full animate-pulse rounded-full bg-accent" />
            </div>
            <div className="min-w-0 flex-1">
              <div className="mb-1 flex items-baseline gap-2">
                <span className="text-xs font-semibold tracking-[0.18em] text-fg">IGRIS</span>
                <span className="text-[10px] text-faint">{streaming.phase === "waiting" ? "Thinking…" : "Responding…"}</span>
              </div>
              {streaming.text ? (
                <div className="stream-caret">
                  <Markdown text={streaming.text} />
                </div>
              ) : (
                <div className="flex gap-1 py-2" aria-label="Thinking">
                  {[0, 1, 2].map((i) => (
                    <span key={i} className="size-1.5 animate-bounce rounded-full bg-faint" style={{ animationDelay: `${i * 0.15}s` }} />
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
