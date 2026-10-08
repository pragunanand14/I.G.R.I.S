import { Copy, Minus, Square, X } from "lucide-react";
import { useEffect, useState } from "react";
import { windowControls } from "@/services/window";

/** Minimise, maximise/restore and close for the frameless window (nothing outside the desktop shell). */
export function WindowControls() {
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

  if (!controls) return null;
  return (
    <div className="ml-1 flex items-center">
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
  );
}

function WindowButton({ label, onClick, danger, children }: { label: string; onClick: () => void; danger?: boolean; children: React.ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={`grid size-8 place-items-center rounded-full text-faint ${danger ? "hover:bg-danger hover:text-white" : "hover:bg-surface-hover hover:text-fg"}`}
    >
      {children}
    </button>
  );
}
