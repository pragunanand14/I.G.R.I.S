import { ArrowUp, Square } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { MicButton } from "./MicButton";
import { VoiceNotice } from "./VoiceNotice";

interface Props {
  onSend: (text: string) => Promise<boolean>;
  onStop: () => void;
  streaming: boolean;
  disabled: boolean;
  disabledReason?: string;
  autoFocus?: boolean;
  placeholder?: string;
  /** Show the push-to-talk button. */
  voice?: boolean;
}

const MAX_HEIGHT = 220;

/** Enter sends, Shift+Enter inserts a newline. Text is restored if sending is rejected. */
export function Composer({ onSend, onStop, streaming, disabled, disabledReason, autoFocus, placeholder = "Ask IGRIS anything…", voice = false }: Props) {
  const [text, setText] = useState("");
  const ref = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, MAX_HEIGHT)}px`;
  }, [text]);

  const submit = async () => {
    const value = text.trim();
    if (!value || disabled || streaming) return;
    setText("");
    const ok = await onSend(value);
    if (!ok) setText((current) => current || value);
    ref.current?.focus();
  };

  return (
    <div>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
        className="flex items-end gap-2 rounded-2xl border border-line bg-elevated/80 py-2 pr-2 pl-4 shadow-[var(--shadow)] backdrop-blur focus-within:border-line-strong"
      >
        <textarea
          ref={ref}
          rows={1}
          value={text}
          autoFocus={autoFocus}
          disabled={disabled}
          onChange={(e) => setText(e.target.value)}
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
            disabled={disabled || !text.trim()}
            aria-label="Send"
            title="Send (Enter)"
            className="grid size-9 shrink-0 place-items-center rounded-xl bg-accent text-bg transition-opacity disabled:cursor-not-allowed disabled:bg-surface-strong disabled:text-faint"
          >
            <ArrowUp className="size-4" />
          </button>
        )}
      </form>
      {voice && <VoiceNotice />}
    </div>
  );
}
