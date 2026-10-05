import type { Attachment } from "./attachments";
// Mirrors `src-tauri/src/conversations` and `src-tauri/src/core/chat.rs`. Keep in sync.
import type { MemoryContext } from "./memory";
import type { ToolActivity } from "./tools";

export type Role = "user" | "assistant";
export type MessageStatus = "complete" | "error" | "cancelled" | "refused" | "truncated";

export interface Conversation {
  id: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  messageCount: number;
}

export interface Message {
  id: string;
  conversationId: string;
  seq: number;
  role: Role;
  content: string;
  status: MessageStatus;
  error: string | null;
  provider: string | null;
  model: string | null;
  inputTokens: number | null;
  outputTokens: number | null;
  createdAt: string;
  toolActivity: ToolActivity[] | null;
  /** User messages: memories attached when sent. */
  memoryContext: MemoryContext | null;
  /** User messages: attached images and PDFs. */
  attachments?: Attachment[];
}

export interface ConversationDetail {
  conversation: Conversation;
  messages: Message[];
  busy: boolean;
}

export type ChatEvent =
  | { type: "userMessage"; conversation: Conversation; message: Message }
  | { type: "generating"; conversationId: string; model: string }
  | { type: "delta"; text: string }
  | { type: "reasoning"; chars: number }
  | { type: "tool"; activity: ToolActivity }
  | { type: "finished"; message: Message };

export interface TurnResult {
  conversationId: string;
  assistantMessage: Message;
}
