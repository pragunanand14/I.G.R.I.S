export interface Attachment {
  id: string;
  kind: "image" | "pdf";
  mime: string;
  name: string;
  size: number;
  width: number | null;
  height: number | null;
  source: "upload" | "screenshot";
  conversationId: string | null;
  messageId: string | null;
  createdAt: string;
}

export interface AttachmentData {
  mime: string;
  data: string;
}

/** Mirrors the backend limits (`attachments/mod.rs`); the backend re-checks by content. */
export const ATTACH_LIMITS = {
  perMessage: 5,
  imageBytes: 5 * 1024 * 1024,
  pdfBytes: 20 * 1024 * 1024,
  accept: "image/png,image/jpeg,image/gif,image/webp,application/pdf",
} as const;
