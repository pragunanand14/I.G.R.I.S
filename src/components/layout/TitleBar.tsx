import { Copy, Minus, Square, X } from "lucide-react";
import { useEffect, useState } from "react";
import { windowControls } from "@/services/window";
import { useAppStore } from "@/stores/appStore";

/** Custom frameless title bar. The whole bar is a drag region (double-click maximises). */
export function TitleBar() {
  const version = useAppStore((s) => s.info?.version);
  const controls = windowControls.available();
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!controls) return;
    let unlisten: (() => void) | undefined;
    const sync = () => void windowControls.isMaximized().then(setMaximized).catch(() => undefined);
    sync();
    void windowControls.onResized(sync).then((fn) => (unlisten = fn));
    return () => unlisten?.();
  }, [controls]);

  return (
    <header data-tauri-drag-region className="flex h-9 shrink-0 items-center justify-between border-b border-line bg-bg/80 pl-4">
      <div data-tauri-drag-region className="flex items-center gap-2.5">
        <svg viewBox="0 0 24 24" className="size-4" aria-hidden="true">
          <circle cx="12" cy="12" r="10" fill="none" stroke="var(--accent)" strokeOpacity="0.4" strokeWidth="1.5" />
          <circle cx="12" cy="12" r="4" fill="var(--accent)" />
        </svg>
        <span data-tauri-drag-region className="text-[11px] font-semibold tracking-[0.32em] text-fg">
          IGRIS
        </span>
        {version && (
          <span data-tauri-drag-region className="font-mono text-[10px] text-faint">
            v{version}
          </span>
        )}
      </div>

      {controls && (
        <div className="flex h-full">
          <WindowButton label="Minimize" onClick={() => void windowControls.minimize()}>
            <Minus className="size-3.5" />
          </WindowButton>
          <WindowButton label={maximized ? "Restore" : "Maximize"} onClick={() => void windowControls.toggleMaximize()}>
            {maximized ? <Copy className="size-3" /> : <Square className="size-3" />}
          </WindowButton>
          <WindowButton label="Close" danger onClick={() => void windowControls.close()}>
            <X className="size-3.5" />
          </WindowButton>
        </div>
      )}
    </header>
  );
}

function WindowButton({
  label,
  onClick,
  danger,
  children,
}: {
  label: string;
  onClick: () => void;
  danger?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={`grid h-full w-11 place-items-center text-muted transition-colors ${
        danger ? "hover:bg-danger hover:text-white" : "hover:bg-surface-hover hover:text-fg"
      }`}
    >
      {children}
    </button>
  );
}
