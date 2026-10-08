import { Copy, Minus, Square, X } from "lucide-react";
import { useEffect, useState } from "react";
import { windowControls } from "@/services/window";
import { useAppStore } from "@/stores/appStore";

/** The frameless window's top strip: a drag region (double-click maximises) with the window buttons. */
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
    <header data-tauri-drag-region className="flex h-10 shrink-0 items-center justify-end" aria-label={version ? `IGRIS v${version}` : "IGRIS"}>
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
      className={`grid h-full w-12 place-items-center text-faint transition-colors duration-200 ${
        danger ? "hover:bg-danger hover:text-white" : "hover:bg-surface-hover hover:text-fg"
      }`}
    >
      {children}
    </button>
  );
}
