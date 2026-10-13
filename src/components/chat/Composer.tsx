import {
  ArrowUp,
  Camera,
  FileText,
  Images,
  Loader2,
  Paperclip,
  Square,
  TriangleAlert,
  X,
} from "lucide-react";
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
  /**
   * The phone layout: bigger box and buttons, Enter starts a new line (the send button sends), and the
   * attach button offers "Take a photo" as well as picking files.
   */
  phone?: boolean;
}

interface Staged {
  key: string;
  name: string;
  isImage: boolean;
  preview: string | null;
  state: "uploading" | "ready" | "error";
  error?: string;
  attachment?: Attachment;
  /** Bytes of the original file (counts toward the per-message total). */
  size: number;
}

const MAX_HEIGHT = 220;
/** The phone shows about six lines before the box scrolls (the keyboard takes the rest). */
const MAX_HEIGHT_PHONE = 160;

/** Client-side pre-check for a fast, clear message; the backend re-validates by content. */
function precheck(file: File): string | null {
  const isPdf =
    file.type === "application/pdf" || file.name.toLowerCase().endsWith(".pdf");
  const isImage = file.type.startsWith("image/");
  if (!isPdf && !isImage)
    return "Only images (PNG, JPEG, GIF, WebP) and PDFs can be attached.";
  const limit = isPdf ? ATTACH_LIMITS.pdfBytes : ATTACH_LIMITS.imageBytes;
  if (file.size > limit)
    return `${isPdf ? "PDFs" : "Images"} can be at most ${limit / 1024 / 1024} MB.`;
  return null;
}

/**
 * Enter sends, Shift+Enter inserts a newline (on the phone, Enter is a new line and the button sends).
 * Text (and files) are restored if sending is rejected.
 */
export function Composer({
  onSend,
  onStop,
  streaming,
  disabled,
  disabledReason,
  autoFocus,
  placeholder = "Ask IGRIS anything…",
  voice = false,
  attachments = false,
  phone = false,
}: Props) {
  const [text, setText] = useState("");
  const [files, setFiles] = useState<Staged[]>([]);
  const [dragging, setDragging] = useState(false);
  const ref = useRef<HTMLTextAreaElement>(null);
  const picker = useRef<HTMLInputElement>(null);
  const camera = useRef<HTMLInputElement>(null);
  const [menu, setMenu] = useState(false);
  const filesRef = useRef(files);
  useEffect(() => {
    filesRef.current = files;
  }, [files]);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, phone ? MAX_HEIGHT_PHONE : MAX_HEIGHT)}px`;
  }, [text, phone]);

  // Free preview URLs on unmount.
  useEffect(
    () => () =>
      filesRef.current.forEach(
        (f) => f.preview && URL.revokeObjectURL(f.preview),
      ),
    [],
  );

  const add = (list: FileList | File[]) => {
    const incoming = Array.from(list);
    const room = ATTACH_LIMITS.perMessage - filesRef.current.length;
    let total = filesRef.current
      .filter((f) => f.state !== "error")
      .reduce((n, f) => n + f.size, 0);
    incoming.forEach((file, i) => {
      const key = crypto.randomUUID();
      const isImage = file.type.startsWith("image/");
      let problem =
        i >= room
          ? `At most ${ATTACH_LIMITS.perMessage} files per message.`
          : precheck(file);
      if (!problem && total + file.size > ATTACH_LIMITS.totalBytes)
        problem = `The files together can be at most ${ATTACH_LIMITS.totalBytes / 1024 / 1024} MB per message.`;
      if (!problem) total += file.size;
      const staged: Staged = {
        key,
        name: file.name || (isImage ? "Pasted image" : "file"),
        isImage,
        preview: isImage && !problem ? URL.createObjectURL(file) : null,
        state: problem ? "error" : "uploading",
        error: problem ?? undefined,
        size: file.size,
      };
      setFiles((fs) => [...fs, staged]);
      if (problem) return;
      api
        .attachFile(file)
        .then((attachment) =>
          setFiles((fs) =>
            fs.map((f) =>
              f.key === key ? { ...f, state: "ready", attachment } : f,
            ),
          ),
        )
        .catch((err) =>
          setFiles((fs) =>
            fs.map((f) =>
              f.key === key
                ? {
                    ...f,
                    state: "error",
                    error: BackendError.from(err).message,
                  }
                : f,
            ),
          ),
        );
    });
  };

  const remove = (key: string) => {
    const f = files.find((x) => x.key === key);
    if (f?.preview) URL.revokeObjectURL(f.preview);
    if (f?.attachment)
      void api.discardAttachment(f.attachment.id).catch(() => undefined);
    setFiles((fs) => fs.filter((x) => x.key !== key));
  };

  const ready = files.filter((f) => f.state === "ready");
  const uploading = files.some((f) => f.state === "uploading");
  const canSend =
    !disabled &&
    !streaming &&
    !uploading &&
    (text.trim().length > 0 || ready.length > 0);

  const submit = async () => {
    if (!canSend) return;
    const value = text.trim();
    const sent = files;
    setText("");
    setFiles([]);
    const ok = await onSend(
      value,
      ready.map((f) => f.attachment!.id),
    );
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
        className={`border bg-surface shadow-[var(--shadow)] ${phone ? "rounded-[28px] p-1.5" : "rounded-[22px] py-2 pr-2 pl-4"} transition-[border-color,box-shadow] duration-300 focus-within:border-line-strong ${dragging ? "border-accent" : "border-line"}`}
      >
        {files.length > 0 && (
          <ul
            className={`mb-2 flex flex-wrap gap-2 pt-1 ${phone ? "px-2" : ""}`}
            aria-label="Attachments"
          >
            {files.map((f) => (
              <li
                key={f.key}
                className={`relative flex items-center gap-2 rounded-lg border bg-surface p-1 pr-7 text-xs ${f.state === "error" ? "border-danger/50" : "border-line"}`}
                title={f.error ?? f.name}
              >
                {f.preview ? (
                  <img
                    src={f.preview}
                    alt=""
                    className="size-10 rounded object-cover"
                  />
                ) : (
                  <span className="grid size-10 place-items-center rounded bg-surface-strong">
                    {f.state === "error" ? (
                      <TriangleAlert className="size-4 text-danger" />
                    ) : (
                      <FileText className="size-4 text-danger" />
                    )}
                  </span>
                )}
                <span className="max-w-36 min-w-0">
                  <span className="block truncate text-fg">{f.name}</span>
                  <span
                    className={`block truncate text-[10px] ${f.state === "error" ? "text-danger" : "text-faint"}`}
                  >
                    {f.state === "uploading"
                      ? "Attaching…"
                      : f.state === "error"
                        ? f.error
                        : f.isImage
                          ? "Image"
                          : "PDF"}
                  </span>
                </span>
                {f.state === "uploading" && (
                  <Loader2 className="absolute top-1/2 right-2 size-3.5 -translate-y-1/2 animate-spin text-muted" />
                )}
                {f.state !== "uploading" && (
                  <button
                    type="button"
                    aria-label={`Remove ${f.name}`}
                    onClick={() => remove(f.key)}
                    className="absolute top-1/2 right-1 -translate-y-1/2 rounded p-1 text-faint hover:text-fg"
                  >
                    <X className="size-3" />
                  </button>
                )}
              </li>
            ))}
          </ul>
        )}
        <div className={`flex items-end ${phone ? "gap-1" : "gap-2"}`}>
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
              {phone && (
                // Opens the phone's camera; the photo comes back attached.
                <input
                  ref={camera}
                  type="file"
                  accept="image/*"
                  capture="environment"
                  className="hidden"
                  aria-hidden
                  tabIndex={-1}
                  onChange={(e) => {
                    if (e.target.files) add(e.target.files);
                    e.target.value = "";
                  }}
                />
              )}
              <div className="relative shrink-0">
                <button
                  type="button"
                  aria-label="Attach images or PDFs"
                  title="Attach images or PDFs"
                  aria-haspopup={phone ? "menu" : undefined}
                  aria-expanded={phone ? menu : undefined}
                  disabled={
                    disabled || files.length >= ATTACH_LIMITS.perMessage
                  }
                  onClick={() =>
                    phone ? setMenu((m) => !m) : picker.current?.click()
                  }
                  className={`grid shrink-0 place-items-center text-muted hover:bg-surface-hover hover:text-fg disabled:opacity-40 ${
                    phone ? "size-11 rounded-full" : "-ml-2 size-9 rounded-xl"
                  }`}
                >
                  <Paperclip className={phone ? "size-[22px]" : "size-4"} />
                </button>
                {phone && menu && (
                  <>
                    <button
                      type="button"
                      aria-hidden
                      tabIndex={-1}
                      className="fixed inset-0 z-10 cursor-default"
                      onClick={() => setMenu(false)}
                    />
                    <div
                      role="menu"
                      className="absolute bottom-full left-0 z-20 mb-2 w-56 overflow-hidden rounded-2xl border border-line bg-surface shadow-[var(--shadow)]"
                    >
                      <button
                        type="button"
                        role="menuitem"
                        className="flex w-full items-center gap-3 px-4 py-3.5 text-left text-[15px] text-fg active:bg-surface-hover"
                        onClick={() => {
                          setMenu(false);
                          camera.current?.click();
                        }}
                      >
                        <Camera className="size-5 text-accent" /> Take a photo
                      </button>
                      <button
                        type="button"
                        role="menuitem"
                        className="flex w-full items-center gap-3 border-t border-line px-4 py-3.5 text-left text-[15px] text-fg active:bg-surface-hover"
                        onClick={() => {
                          setMenu(false);
                          picker.current?.click();
                        }}
                      >
                        <Images className="size-5 text-accent" /> Photos and
                        files
                      </button>
                    </div>
                  </>
                )}
              </div>
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
            enterKeyHint={phone ? "enter" : "send"}
            onKeyDown={(e) => {
              if (
                !phone &&
                e.key === "Enter" &&
                !e.shiftKey &&
                !e.nativeEvent.isComposing
              ) {
                e.preventDefault();
                void submit();
              }
            }}
            aria-label="Message IGRIS"
            placeholder={
              disabled && disabledReason ? disabledReason : placeholder
            }
            className={`min-w-0 flex-1 resize-none bg-transparent text-fg placeholder:text-faint focus:outline-none disabled:cursor-not-allowed ${
              phone
                ? "max-h-[160px] min-h-11 px-1 py-2.5 text-base leading-6"
                : "max-h-[220px] min-h-9 py-2 text-sm leading-5"
            }`}
          />
          {voice && <MicButton disabled={disabled} large={phone} />}
          {streaming ? (
            <button
              type="button"
              onClick={onStop}
              aria-label="Stop responding"
              title="Stop"
              className={`grid shrink-0 place-items-center bg-surface-strong text-fg transition-colors hover:bg-danger hover:text-white ${phone ? "size-11 rounded-full" : "size-9 rounded-xl"}`}
            >
              <Square
                className={`${phone ? "size-4" : "size-3.5"} fill-current`}
              />
            </button>
          ) : (
            <button
              type="submit"
              disabled={!canSend}
              aria-label="Send"
              title={
                uploading
                  ? "Waiting for attachments…"
                  : phone
                    ? "Send"
                    : "Send (Enter)"
              }
              className={`grid shrink-0 place-items-center bg-accent text-bg transition-opacity disabled:cursor-not-allowed disabled:bg-surface-strong disabled:text-faint ${
                phone ? "size-11 rounded-full" : "size-9 rounded-xl"
              }`}
            >
              <ArrowUp className={phone ? "size-[22px]" : "size-4"} />
            </button>
          )}
        </div>
      </form>
      {voice && <VoiceNotice />}
    </div>
  );
}
