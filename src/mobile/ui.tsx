import { ChevronLeft } from "lucide-react";
import { useEffect, type ReactNode } from "react";
import { useNavigate } from "react-router";

/** A phone screen: a large title, optional back button and action, then scrolling content. */
export function Screen({
  title,
  subtitle,
  back,
  action,
  children,
}: {
  title: string;
  subtitle?: string;
  /** Where the back button goes (shown only when set). */
  back?: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  const navigate = useNavigate();
  return (
    <div className="mx-auto w-full max-w-xl px-5 pt-5 pb-8">
      {back && (
        <button type="button" onClick={() => void navigate(back)} className="m-muted -ml-2 mb-2 flex min-h-11 items-center gap-1 pr-3 text-[15px] font-medium">
          <ChevronLeft className="size-5" /> Back
        </button>
      )}
      <header className="mb-5 flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h1 className="m-title">{title}</h1>
          {subtitle && <p className="m-muted mt-1 text-[15px]">{subtitle}</p>}
        </div>
        {action}
      </header>
      <div className="space-y-5">{children}</div>
    </div>
  );
}

/** A titled group of content. */
export function Section({ title, children, aside }: { title: string; children: ReactNode; aside?: ReactNode }) {
  return (
    <section>
      <div className="mb-2 flex items-center justify-between px-1">
        <h2 className="m-section-title">{title}</h2>
        {aside}
      </div>
      {children}
    </section>
  );
}

/** A bottom sheet. */
export function Sheet({ open, onClose, title, children }: { open: boolean; onClose: () => void; title: string; children: ReactNode }) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);
  if (!open) return null;
  return (
    <>
      <div className="m-sheet-backdrop" onClick={onClose} aria-hidden="true" />
      <div className="m-sheet" role="dialog" aria-modal="true" aria-label={title}>
        <div className="m-sheet-handle" />
        <h2 className="mb-3 text-lg font-semibold">{title}</h2>
        {children}
      </div>
    </>
  );
}

export type Tone = "ok" | "warn" | "bad";

const TONE_COLOR: Record<Tone, string> = { ok: "var(--m-ok)", warn: "var(--m-warn)", bad: "var(--m-bad)" };

export function Dot({ tone }: { tone: Tone }) {
  return <span className="inline-block size-2.5 shrink-0 rounded-full" style={{ background: TONE_COLOR[tone] }} aria-hidden="true" />;
}

/** A plain on/off switch with a label and an explanation. */
export function Switch({
  label,
  description,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  description?: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <label className="m-row cursor-pointer">
      <div className="min-w-0 flex-1">
        <div className="font-medium">{label}</div>
        {description && <div className="m-muted mt-0.5 text-sm">{description}</div>}
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className="relative h-8 w-13 shrink-0 rounded-full transition-colors disabled:opacity-40"
        style={{ background: checked ? "var(--m-accent)" : "var(--m-card-2)", border: "1px solid var(--m-line)" }}
      >
        <span
          className="absolute top-1/2 size-6 -translate-y-1/2 rounded-full bg-white shadow transition-[left]"
          style={{ left: checked ? "calc(100% - 28px)" : "3px" }}
        />
      </button>
    </label>
  );
}
