import { Channel } from "@tauri-apps/api/core";
import type { ChatEvent, TurnResult } from "@/types/chat";
import { call } from "./backend";

/** Streaming chat commands. Events arrive on a Tauri channel while the promise is pending. */
type Start =
  | { kind: "send"; conversationId: string | null; content: string; attachmentIds?: string[] }
  | { kind: "regenerate"; conversationId: string }
  | { kind: "edit"; messageId: string; content: string };

export function startTurn(requestId: string, start: Start, onEvent: (ev: ChatEvent) => void): Promise<TurnResult> {
  const channel = new Channel<ChatEvent>();
  channel.onmessage = onEvent;
  switch (start.kind) {
    case "send":
      return call<TurnResult>("chat_send", {
        requestId,
        conversationId: start.conversationId,
        content: start.content,
        attachmentIds: start.attachmentIds ?? [],
        onEvent: channel,
      });
    case "regenerate":
      return call<TurnResult>("chat_regenerate", { requestId, conversationId: start.conversationId, onEvent: channel });
    case "edit":
      return call<TurnResult>("chat_edit", { requestId, messageId: start.messageId, content: start.content, onEvent: channel });
  }
}

export function newRequestId(): string {
  return crypto.randomUUID();
}
