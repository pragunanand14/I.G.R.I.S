import type { ReactNode } from "react";

interface PanelProps {
  title?: ReactNode;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
}

export function Panel({ title, action, children, className = "" }: PanelProps) {
  return (
    <section className={`rounded-xl border border-line bg-surface backdrop-blur-sm ${className}`}>
      {(title || action) && (
        <header className="flex items-center justify-between gap-4 border-b border-line px-4 py-2.5">
          {title && <h2 className="text-label">{title}</h2>}
          {action}
        </header>
      )}
      <div className="p-4">{children}</div>
    </section>
  );
}
