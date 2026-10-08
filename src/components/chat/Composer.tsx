import { ArrowUp, FileText, Loader2, Paperclip, Square, TriangleAlert, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { ATTACH_LIMITS, type Attachment } from "@/types/attachments";
import { MicButton } from "./MicButton";
import { VoiceNotice } from "./VoiceNotice";

interface Props {
  onSend: (text: string, attachmentIds: string[]) => Promise<boolean>;
  onStop: () => void;
  streaming: boolean;
  disabled: boolean;
  disabledReason?: string;
  autoFocus?: boolean;
  placeholder?: string;
  /** Show the push-to-talk button. */
  voice?: boolean;
  /** Allow attaching images and PDFs. */
  attachments?: boolean;
}

interface Staged {
  key: string;
  name: string;
  isImage: boolean;
  preview: string | null;
  state: "uploading" | "ready" | "error";
  error?: string;
  attachment?: Attachment;
}

const MAX_HEIGHT = 220;

/** Client-side pre-check for a fast, clear message; the backend re-validates by content. */
function precheck(file: File): string | null {
  const isPdf = file.type === "application/pdf" || file.name.toLowerCase().endsWith(".pdf");
  const isImage = file.type.startsWith("image/");
  if (!isPdf && !isImage) return "Only images (PNG, JPEG, GIF, WebP) and PDFs can be attached.";
  const limit = isPdf ? ATTACH_LIMITS.pdfBytes : ATTACH_LIMITS.imageBytes;
  if (file.size > limit) return `${isPdf ? "PDFs" : "Images"} can be at most ${limit / 1024 / 1024} MB.`;
  return null;
}

/** Enter sends, Shift+Enter inserts a newline. Text (and files) are restored if sending is rejected. */
export function Composer({ onSend, onStop, streaming, disabled, disabledReason, autoFocus, placeholder = "Ask IGRIS anything…", voice = false, attachments = false }: Props) {
  const [text, setText] = useState("");
  const [files, setFiles] = useState<Staged[]>([]);
  const [dragging, setDragging] = useState(false);
  const ref = useRef<HTMLTextAreaElement>(null);
  const picker = useRef<HTMLInputElement>(null);
  const filesRef = useRef(files);
  useEffect(() => {
    filesRef.current = files;
  }, [files]);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, MAX_HEIGHT)}px`;
  }, [text]);

  // Free preview URLs on unmount.
  useEffect(() => () => filesRef.current.forEach((f) => f.preview && URL.revokeObjectURL(f.preview)), []);

  const add = (list: FileList | File[]) => {
    const incoming = Array.from(list);
    const room = ATTACH_LIMITS.perMessage - filesRef.current.length;
    incoming.forEach((file, i) => {
      const key = crypto.randomUUID();
      const isImage = file.type.startsWith("image/");
      const problem = i >= room ? `At most ${ATTACH_LIMITS.perMessage} files per message.` : precheck(file);
      const staged: Staged = { key, name: file.name || (isImage ? "Pasted image" : "file"), isImage, preview: isImage && !problem ? URL.createObjectURL(file) : null, state: problem ? "error" : "uploading", error: problem ?? undefined };
      setFiles((fs) => [...fs, staged]);
      if (problem) return;
      api
        .attachFile(file)
        .then((attachment) => setFiles((fs) => fs.map((f) => (f.key === key ? { ...f, state: "ready", attachment } : f))))
        .catch((err) => setFiles((fs) => fs.map((f) => (f.key === key ? { ...f, state: "error", error: BackendError.from(err).message } : f))));
    });
  };

  const remove = (key: string) => {
    const f = files.find((x) => x.key === key);
    if (f?.preview) URL.revokeObjectURL(f.preview);
    if (f?.attachment) void api.discardAttachment(f.attachment.id).catch(() => undefined);
    setFiles((fs) => fs.filter((x) => x.key !== key));
  };

  const ready = files.filter((f) => f.state === "ready");
  const uploading = files.some((f) => f.state === "uploading");
  const canSend = !disabled && !streaming && !uploading && (text.trim().length > 0 || ready.length > 0);

  const submit = async () => {
    if (!canSend) return;
    const value = text.trim();
    const sent = files;
    setText("");
    setFiles([]);
    const ok = await onSend(value, ready.map((f) => f.attachment!.id));
    if (!ok) {
      setText((current) => current || value);
      setFiles((current) => (current.length ? current : sent));
    } else {
      sent.forEach((f) => f.preview && URL.revokeObjectURL(f.preview));
    }
    ref.current?.focus();
  };

  return (
    <div
      onDragOver={
        attachments
          ? (e) => {
              if (Array.from(e.dataTransfer.types).includes("Files")) {
                e.preventDefault();
                setDragging(true);
              }
            }
          : undefined
      }
      onDragLeave={attachments ? () => setDragging(false) : undefined}
      onDrop={
        attachments
          ? (e) => {
              if (e.dataTransfer.files.length) {
                e.preventDefault();
                add(e.dataTransfer.files);
              }
              setDragging(false);
            }
          : undefined
      }
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
        className={`rounded-[22px] border bg-surface py-2 pr-2 pl-4 shadow-[var(--shadow)] transition-[border-color,box-shadow] duration-300 focus-within:border-line-strong ${dragging ? "border-accent" : "border-line"}`}
      >
        {files.length > 0 && (
          <ul className="mb-2 flex flex-wrap gap-2 pt-1" aria-label="Attachments">
            {files.map((f) => (
              <li key={f.key} className={`relative flex items-center gap-2 rounded-lg border bg-surface p-1 pr-7 text-xs ${f.state === "error" ? "border-danger/50" : "border-line"}`} title={f.error ?? f.name}>
                {f.preview ? (
                  <img src={f.preview} alt="" className="size-10 rounded object-cover" />
                ) : (
                  <span className="grid size-10 place-items-center rounded bg-surface-strong">
                    {f.state === "error" ? <TriangleAlert className="size-4 text-danger" /> : <FileText className="size-4 text-danger" />}
                  </span>
                )}
                <span className="max-w-36 min-w-0">
                  <span className="block truncate text-fg">{f.name}</span>
                  <span className={`block truncate text-[10px] ${f.state === "error" ? "text-danger" : "text-faint"}`}>
                    {f.state === "uploading" ? "Attaching…" : f.state === "error" ? f.error : f.isImage ? "Image" : "PDF"}
                  </span>
                </span>
                {f.state === "uploading" && <Loader2 className="absolute top-1/2 right-2 size-3.5 -translate-y-1/2 animate-spin text-muted" />}
                {f.state !== "uploading" && (
                  <button type="button" aria-label={`Remove ${f.name}`} onClick={() => remove(f.key)} className="absolute top-1/2 right-1 -translate-y-1/2 rounded p-1 text-faint hover:text-fg">
                    <X className="size-3" />
                  </button>
                )}
              </li>
            ))}
          </ul>
        )}
        <div className="flex items-end gap-2">
          {attachments && (
            <>
              <input
                ref={picker}
                type="file"
                multiple
                accept={ATTACH_LIMITS.accept}
                className="hidden"
                aria-hidden
                tabIndex={-1}
                onChange={(e) => {
                  if (e.target.files) add(e.target.files);
                  e.target.value = "";
                }}
              />
              <button
                type="button"
                aria-label="Attach images or PDFs"
                title="Attach images or PDFs"
                disabled={disabled || files.length >= ATTACH_LIMITS.perMessage}
                onClick={() => picker.current?.click()}
                className="-ml-2 grid size-9 shrink-0 place-items-center rounded-xl text-muted hover:bg-surface-hover hover:text-fg disabled:opacity-40"
              >
                <Paperclip className="size-4" />
              </button>
            </>
          )}
          <textarea
            ref={ref}
            rows={1}
            value={text}
            autoFocus={autoFocus}
            disabled={disabled}
            onChange={(e) => setText(e.target.value)}
            onPaste={
              attachments
                ? (e) => {
                    const pasted = Array.from(e.clipboardData.files);
                    if (pasted.length) {
                      e.preventDefault();
                      add(pasted);
                    }
                  }
                : undefined
            }
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                void submit();
              }
            }}
            aria-label="Message IGRIS"
            placeholder={disabled && disabledReason ? disabledReason : placeholder}
            className="max-h-[220px] min-h-9 min-w-0 flex-1 resize-none bg-transparent py-2 text-sm leading-5 text-fg placeholder:text-faint focus:outline-none disabled:cursor-not-allowed"
          />
          {voice && <MicButton disabled={disabled} />}
          {streaming ? (
            <button
              type="button"
              onClick={onStop}
              aria-label="Stop responding"
              title="Stop"
              className="grid size-9 shrink-0 place-items-center rounded-xl bg-surface-strong text-fg transition-colors hover:bg-danger hover:text-white"
            >
              <Square className="size-3.5 fill-current" />
            </button>
          ) : (
            <button
              type="submit"
              disabled={!canSend}
              aria-label="Send"
              title={uploading ? "Waiting for attachments…" : "Send (Enter)"}
              className="grid size-9 shrink-0 place-items-center rounded-xl bg-accent text-bg transition-opacity disabled:cursor-not-allowed disabled:bg-surface-strong disabled:text-faint"
            >
              <ArrowUp className="size-4" />
            </button>
          )}
        </div>
      </form>
      {voice && <VoiceNotice />}
    </div>
  );
}
