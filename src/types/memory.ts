// Mirrors `src-tauri/src/memory`. Keep in sync.
export type MemoryKind = "long_term" | "knowledge";

export interface Memory {
  id: number;
  kind: MemoryKind;
  content: string;
  source: "user" | "assistant";
  conversationId: string | null;
  createdAt: string;
  updatedAt: string;
  lastUsedAt: string | null;
  useCount: number;
}

export interface AttachedMemory {
  id: number;
  kind: MemoryKind;
  content: string;
  updatedAt: string;
}

export interface MemoryContext {
  items: AttachedMemory[];
  rendered: string;
}

export const MEMORY_KIND_LABEL: Record<MemoryKind, string> = {
  long_term: "About you",
  knowledge: "Knowledge",
};

export const MEMORY_MAX_CHARS = 500;
