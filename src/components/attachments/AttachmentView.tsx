import { FileText, ImageOff, X } from "lucide-react";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import type { Attachment } from "@/types/attachments";
import { attachmentUrl, formatSize } from "./attachmentCache";

function Lightbox({ src, alt, onClose }: { src: string; alt: string; onClose: () => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return createPortal(
    <div role="dialog" aria-label={alt} className="fixed inset-0 z-50 grid place-items-center bg-black/80 p-8" onClick={onClose}>
      <button type="button" aria-label="Close" className="absolute top-4 right-4 rounded-full bg-black/50 p-2 text-white" onClick={onClose}>
        <X className="size-4" />
      </button>
      <img src={src} alt={alt} className="max-h-full max-w-full rounded-lg object-contain shadow-2xl" onClick={(e) => e.stopPropagation()} />
    </div>,
    document.body,
  );
}

/** A stored image, loaded from the backend; click to enlarge. */
export function StoredImage({ id, name, className = "h-24" }: { id: string; name: string; className?: string }) {
  const [state, setState] = useState<{ id: string; src: string | null; failed: boolean }>({ id, src: null, failed: false });
  const [open, setOpen] = useState(false);
  useEffect(() => {
    let live = true;
    attachmentUrl(id)
      .then((src) => live && setState({ id, src, failed: false }))
      .catch(() => live && setState({ id, src: null, failed: true }));
    return () => {
      live = false;
    };
  }, [id]);
  const current = state.id === id ? state : { src: null, failed: false };
  if (current.failed) {
    return (
      <span className={`flex aspect-video items-center justify-center gap-1 rounded-lg border border-line px-3 text-[11px] text-faint ${className}`}>
        <ImageOff className="size-3.5" /> Unavailable
      </span>
    );
  }
  if (!current.src) return <span className={`block aspect-video animate-pulse rounded-lg bg-surface-strong ${className}`} />;
  return (
    <>
      <button type="button" onClick={() => setOpen(true)} title={name} className="block overflow-hidden rounded-lg border border-line hover:border-accent">
        <img src={current.src} alt={name} className={`w-auto object-cover ${className}`} />
      </button>
      {open && <Lightbox src={current.src} alt={name} onClose={() => setOpen(false)} />}
    </>
  );
}

export function PdfChip({ name, size }: { name: string; size?: number }) {
  return (
    <span className="flex max-w-56 items-center gap-2 rounded-lg border border-line bg-surface px-2.5 py-2 text-xs text-fg" title={name}>
      <FileText className="size-4 shrink-0 text-danger" />
      <span className="min-w-0">
        <span className="block truncate">{name}</span>
        {size !== undefined && <span className="text-[10px] text-faint">PDF · {formatSize(size)}</span>}
      </span>
    </span>
  );
}

/** Attachments shown on a sent message. */
export function AttachmentList({ items, align = "end" }: { items: Attachment[]; align?: "start" | "end" }) {
  if (items.length === 0) return null;
  return (
    <div className={`flex flex-wrap gap-2 ${align === "end" ? "justify-end" : ""}`}>
      {items.map((a) => (a.kind === "image" ? <StoredImage key={a.id} id={a.id} name={a.name} /> : <PdfChip key={a.id} name={a.name} size={a.size} />))}
    </div>
  );
}
