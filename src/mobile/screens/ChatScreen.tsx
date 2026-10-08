import { ChevronLeft, MessagesSquare, SquarePen, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { AiCore } from "@/components/core/AiCore";
import { Composer } from "@/components/chat/Composer";
import { MessageList } from "@/components/chat/MessageList";
import { TaskCard } from "@/components/chat/TaskCard";
import { useAppStore } from "@/stores/appStore";
import { useChatStore } from "@/stores/chatStore";
import { useTaskStore } from "@/stores/taskStore";
import { useCoreState } from "@/hooks/useCoreState";
import { nav } from "../motion";
import { useStatus } from "../status";
import { SUGGESTIONS, useAsk } from "../ask";
import { whenText } from "../format";
import { Dot, Screen } from "../ui";

/** One conversation, full screen; past chats are one tap away. */
export function ChatScreen() {
  const navigate = useNavigate();
  const core = useCoreState();
  const backend = useAppStore((s) => s.backend);
  const status = useStatus();
  const s = useChatStore();
  const { loadConversations } = s;
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
      <header className="flex shrink-0 items-center gap-1 px-2 pt-2 pb-1">
        <button type="button" onClick={() => void navigate("/", nav())} className="m-icon-button" aria-label="Home">
          <ChevronLeft className="size-6" />
        </button>
        <div className="flex min-w-0 flex-1 items-center justify-center gap-2">
          <span className="m-chat-orb" aria-hidden="true">
            <AiCore state={core} size="100%" />
          </span>
          <h1 className="min-w-0 truncate text-base font-semibold">{active?.title ?? "New chat"}</h1>
        </div>
        <button type="button" onClick={() => void navigate("/chats", nav())} className="m-icon-button" aria-label="Your chats">
          <MessagesSquare className="size-[22px]" />
        </button>
        <button type="button" onClick={() => void s.openConversation(null)} className="m-icon-button" aria-label="New chat">
          <SquarePen className="size-[22px]" />
        </button>
      </header>

      {!status.ready && !streamingHere && (
        <div className="m-card mx-4 mt-2 px-4 py-3" role="status">
          <p className="flex items-center gap-2 text-[15px] font-medium">
            <Dot tone={status.tone} /> {status.title}
          </p>
          <p className="m-muted mt-0.5 text-sm break-words">{status.detail}</p>
        </div>
      )}

      {s.error && (
        <div role="alert" className="m-card mx-4 mt-2 px-4 py-3 text-sm" style={{ color: "var(--m-bad)" }}>
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
            <div className="mx-auto flex h-full max-w-xl flex-col justify-end px-5 pb-4">
              <p className="text-2xl font-semibold tracking-tight">What can I do for you?</p>
              <p className="m-muted mt-2 text-[15px]">Questions, web searches, reminders and to-dos, opening your apps, and remembering things for you.</p>
              <div className="m-group mt-6">
                {SUGGESTIONS.map((x) => (
                  <button key={x} type="button" className="m-row min-h-12 text-[15px] disabled:opacity-40" disabled={!status.ready} onClick={() => void ask(x)}>
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

      <div className="mx-auto w-full max-w-xl shrink-0 px-3 pt-1 pb-2">
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
    </div>
  );
}

/** Your chats: open one, start a new one, delete old ones. */
export function ChatsScreen() {
  const navigate = useNavigate();
  const backend = useAppStore((s) => s.backend);
  const load = useChatStore((s) => s.loadConversations);
  const open = useChatStore((s) => s.openConversation);
  useEffect(() => {
    if (backend === "ready") void load();
  }, [backend, load]);
  return (
    <Screen title="Chats" back="/">
      <ChatList
        onPick={(id) => {
          void open(id);
          void navigate("/chat", nav());
        }}
      />
    </Screen>
  );
}

function ChatList({ onPick }: { onPick: (id: string | null) => void }) {
  const conversations = useChatStore((s) => s.conversations);
  const activeId = useChatStore((s) => s.activeId);
  const remove = useChatStore((s) => s.remove);
  const [confirm, setConfirm] = useState<string | null>(null);
  return (
    <div className="space-y-4">
      <button type="button" className="m-button m-button-primary w-full" onClick={() => onPick(null)}>
        <SquarePen className="size-5" /> New chat
      </button>
      {conversations.length === 0 ? (
        <p className="m-muted py-4 text-center">No chats yet.</p>
      ) : (
        <ul className="m-group" aria-label="Chats">
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
                  <button type="button" className="m-icon-button -mr-2" aria-label={`Delete ${c.title}`} onClick={() => setConfirm(c.id)}>
                    <Trash2 className="m-faint size-[18px]" />
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
