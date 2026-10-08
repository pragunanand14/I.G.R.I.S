import type { ReactNode } from "react";

interface PanelProps {
  title?: ReactNode;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
}

/** A quiet surface for one group of content: a small title, then the content. */
export function Panel({ title, action, children, className = "" }: PanelProps) {
  return (
    <section className={`anim-rise rounded-2xl border border-line bg-surface transition-colors duration-300 ${className}`}>
      {(title || action) && (
        <header className="flex min-h-11 items-center justify-between gap-4 px-5 pt-3">
          {title && <h2 className="text-[13px] font-medium text-muted">{title}</h2>}
          {action}
        </header>
      )}
      <div className={title || action ? "px-5 pt-2 pb-5" : "p-5"}>{children}</div>
    </section>
  );
}
