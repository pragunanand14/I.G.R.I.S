import type { ReactNode } from "react";

interface PanelProps {
  title?: ReactNode;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
}

/** One group of content on a soft, borderless surface: a title, then the content. */
export function Panel({ title, action, children, className = "" }: PanelProps) {
  return (
    <section className={`anim-rise rounded-[20px] bg-surface ${className}`}>
      {(title || action) && (
        <header className="flex min-h-12 items-center justify-between gap-4 px-6 pt-4">
          {title && <h2 className="text-[15px] font-semibold text-fg">{title}</h2>}
          {action}
        </header>
      )}
      <div className={title || action ? "px-6 pt-2 pb-6" : "p-6"}>{children}</div>
    </section>
  );
}
