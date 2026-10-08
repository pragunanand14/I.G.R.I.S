import { ChevronLeft, ChevronRight } from "lucide-react";
import { useEffect, type ReactNode } from "react";
import { Link, useNavigate } from "react-router";

/** A phone screen: optional back button, a large title, then scrolling content. */
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
    <div className="mx-auto w-full max-w-xl px-5 pt-4 pb-10">
      {back || action ? (
        <div className="mb-3 flex min-h-11 items-center justify-between">
          {back ? (
            <button type="button" onClick={() => void navigate(back)} className="m-icon-button -ml-3" aria-label="Back">
              <ChevronLeft className="size-6" />
            </button>
          ) : (
            <span />
          )}
          {action}
        </div>
      ) : (
        <div className="h-6" />
      )}
      <header className="mb-7">
        <h1 className="m-title">{title}</h1>
        {subtitle && <p className="m-muted mt-1.5 text-[15px]">{subtitle}</p>}
      </header>
      <div className="space-y-7">{children}</div>
    </div>
  );
}

/** A group of content with a small label above it. */
export function Section({ title, children, aside }: { title?: string; children: ReactNode; aside?: ReactNode }) {
  return (
    <section className="space-y-2">
      {(title || aside) && (
        <div className="flex min-h-6 items-center justify-between">
          {title ? <h2 className="m-label">{title}</h2> : <span />}
          {aside}
        </div>
      )}
      {children}
    </section>
  );
}

/** A grouped list. */
export function Group({ children, label }: { children: ReactNode; label?: string }) {
  return (
    <div className="m-group" role={label ? "list" : undefined} aria-label={label}>
      {children}
    </div>
  );
}

/** One row of a group: a title, an optional line under it, an optional value on the right; a link when `to` is set. */
export function Row({
  title,
  detail,
  value,
  to,
  onClick,
  icon,
}: {
  title: ReactNode;
  detail?: ReactNode;
  value?: ReactNode;
  to?: string;
  onClick?: () => void;
  icon?: ReactNode;
}) {
  const body = (
    <>
      {icon && <span className="m-muted shrink-0">{icon}</span>}
      <div className="min-w-0 flex-1">
        <div className="font-medium break-words">{title}</div>
        {detail && <div className="m-muted mt-0.5 text-sm break-words">{detail}</div>}
      </div>
      {value !== undefined && <span className="m-muted shrink-0 text-[15px]">{value}</span>}
      {(to || onClick) && <ChevronRight className="m-faint size-5 shrink-0" />}
    </>
  );
  if (to)
    return (
      <Link to={to} className="m-row">
        {body}
      </Link>
    );
  if (onClick)
    return (
      <button type="button" className="m-row" onClick={onClick}>
        {body}
      </button>
    );
  return <div className="m-row">{body}</div>;
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
        <h2 className="mb-4 text-xl font-semibold">{title}</h2>
        {children}
      </div>
    </>
  );
}

export type Tone = "ok" | "warn" | "bad";

const TONE_COLOR: Record<Tone, string> = { ok: "var(--m-ok)", warn: "var(--m-warn)", bad: "var(--m-bad)" };

export function Dot({ tone }: { tone: Tone }) {
  return <span className="inline-block size-2 shrink-0 rounded-full" style={{ background: TONE_COLOR[tone] }} aria-hidden="true" />;
}

/** A short error line. */
export function ErrorText({ children }: { children: ReactNode }) {
  return (
    <p role="alert" className="px-1 text-sm" style={{ color: "var(--m-bad)" }}>
      {children}
    </p>
  );
}

/** A plain on/off switch with a label and an explanation, as a group row. */
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
        className="relative h-[30px] w-[50px] shrink-0 rounded-full transition-colors disabled:opacity-40"
        style={{ background: checked ? "var(--m-ok)" : "var(--m-surface-2)" }}
      >
        <span
          className="absolute top-1/2 size-[26px] -translate-y-1/2 rounded-full bg-white shadow-sm transition-[left]"
          style={{ left: checked ? "calc(100% - 28px)" : "2px" }}
        />
      </button>
    </label>
  );
}
