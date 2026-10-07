import { MessagesSquare, SquarePen, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { Composer } from "@/components/chat/Composer";
import { MessageList } from "@/components/chat/MessageList";
import { TaskCard } from "@/components/chat/TaskCard";
import { useAppStore } from "@/stores/appStore";
import { useChatStore } from "@/stores/chatStore";
import { useTaskStore } from "@/stores/taskStore";
import { useStatus } from "../status";
import { SUGGESTIONS, useAsk } from "../ask";
import { whenText } from "../format";
import { Dot, Sheet } from "../ui";

/** One conversation, full screen; past chats are a sheet away. */
export function ChatScreen() {
  const backend = useAppStore((s) => s.backend);
  const status = useStatus();
  const s = useChatStore();
  const { loadConversations } = s;
  const [listOpen, setListOpen] = useState(false);
  const ask = useAsk();

  useEffect(() => {
    if (backend === "ready") void loadConversations();
  }, [backend, loadConversations]);

  const loadTasks = useTaskStore((t) => t.load);
  useEffect(() => {
    if (backend === "ready" && s.activeId) void loadTasks(s.activeId);
  }, [backend, s.activeId, loadTasks]);

  const active = s.conversations.find((c) => c.id === s.activeId) ?? null;
  const streamingHere = s.streaming !== null && (s.streaming.conversationId === s.activeId || s.streaming.conversationId === null);
  const streamingElsewhere = s.streaming !== null && !streamingHere;
  const disabledReason = !status.ready && !streamingHere ? status.title : streamingElsewhere ? "IGRIS is answering in another chat…" : undefined;
  const empty = s.messages.length === 0 && !s.pendingUser && !streamingHere;

  return (
    <div className="m-chat-thread flex h-full flex-col">
      <header className="flex shrink-0 items-center gap-2 px-3 pt-3 pb-2" style={{ borderBottom: "1px solid var(--m-line)" }}>
        <button type="button" onClick={() => setListOpen(true)} className="m-button min-h-11 px-3" aria-label="Your chats">
          <MessagesSquare className="size-5" />
        </button>
        <h1 className="min-w-0 flex-1 truncate text-center text-[17px] font-semibold">{active?.title ?? "New chat"}</h1>
        <button type="button" onClick={() => void s.openConversation(null)} className="m-button min-h-11 px-3" aria-label="New chat">
          <SquarePen className="size-5" />
        </button>
      </header>

      {!status.ready && !streamingHere && (
        <div className="m-card mx-4 mt-3 flex items-start gap-3 p-3" role="status">
          <span className="mt-1.5">
            <Dot tone={status.tone} />
          </span>
          <div className="min-w-0">
            <p className="font-semibold">{status.title}</p>
            <p className="m-muted text-sm break-words">{status.detail}</p>
          </div>
        </div>
      )}

      {s.error && (
        <div role="alert" className="m-card mx-4 mt-3 p-3 text-sm" style={{ color: "var(--m-bad)" }}>
          {s.error}{" "}
          <button type="button" onClick={s.dismissError} className="ml-1 font-semibold underline">
            OK
          </button>
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto">
        {empty ? (
          s.loading ? (
            <p className="m-muted p-6 text-center">Loading…</p>
          ) : (
            <div className="mx-auto max-w-xl px-5 pt-10 text-center">
              <p className="text-xl font-semibold">What can I do for you?</p>
              <p className="m-muted mt-2">
                I can answer questions, search the web, set reminders and to-dos, open your apps, check your battery and remember things for you.
              </p>
              <div className="mt-6 flex flex-col items-stretch gap-2">
                {SUGGESTIONS.map((x) => (
                  <button key={x} type="button" className="m-chip justify-center" disabled={!status.ready} onClick={() => void ask(x)}>
                    {x}
                  </button>
                ))}
              </div>
            </div>
          )
        ) : (
          <MessageList
            messages={s.messages}
            pendingUser={streamingHere ? s.pendingUser : null}
            streaming={streamingHere ? s.streaming : null}
            onRegenerate={() => void s.regenerate()}
            onEdit={s.edit}
            onAnswerApproval={(id, ok, trust) => void s.answerApproval(id, ok, trust)}
          />
        )}
      </div>

      <div className="mx-auto w-full max-w-xl shrink-0 px-3 pt-1 pb-3">
        <TaskCard conversationId={s.activeId} busy={s.streaming !== null} onResume={(id, cid) => void s.resumeTask(id, cid)} />
        <Composer
          key={s.activeId ?? "new"}
          onSend={s.send}
          onStop={() => void s.cancel()}
          streaming={streamingHere}
          disabled={disabledReason !== undefined}
          disabledReason={disabledReason}
          placeholder="Message IGRIS…"
          voice={backend === "ready"}
          attachments={backend === "ready"}
        />
      </div>

      <Sheet open={listOpen} onClose={() => setListOpen(false)} title="Your chats">
        <ChatList
          onPick={(id) => {
            setListOpen(false);
            void s.openConversation(id);
          }}
        />
      </Sheet>
    </div>
  );
}

function ChatList({ onPick }: { onPick: (id: string | null) => void }) {
  const conversations = useChatStore((s) => s.conversations);
  const activeId = useChatStore((s) => s.activeId);
  const remove = useChatStore((s) => s.remove);
  const [confirm, setConfirm] = useState<string | null>(null);
  return (
    <div className="space-y-3">
      <button type="button" className="m-button m-button-primary w-full" onClick={() => onPick(null)}>
        <SquarePen className="size-5" /> Start a new chat
      </button>
      {conversations.length === 0 ? (
        <p className="m-muted py-4 text-center">No chats yet.</p>
      ) : (
        <ul className="m-card" aria-label="Chats">
          {conversations.map((c) => (
            <li key={c.id} className="m-row">
              {confirm === c.id ? (
                <>
                  <span className="min-w-0 flex-1 text-sm">Delete “{c.title}”?</span>
                  <button type="button" className="m-button min-h-10 px-3 text-sm" onClick={() => setConfirm(null)}>
                    Keep
                  </button>
                  <button
                    type="button"
                    className="m-button min-h-10 px-3 text-sm"
                    style={{ color: "var(--m-bad)" }}
                    onClick={() => void remove(c.id).then(() => setConfirm(null))}
                  >
                    Delete
                  </button>
                </>
              ) : (
                <>
                  <button type="button" className="min-w-0 flex-1 text-left" onClick={() => onPick(c.id)} aria-current={c.id === activeId ? "true" : undefined}>
                    <div className={`truncate font-medium ${c.id === activeId ? "m-accent" : ""}`}>{c.title}</div>
                    <div className="m-muted text-sm">{whenText(c.updatedAt)}</div>
                  </button>
                  <button type="button" className="m-faint grid size-11 place-items-center" aria-label={`Delete ${c.title}`} onClick={() => setConfirm(c.id)}>
                    <Trash2 className="size-5" />
                  </button>
                </>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
