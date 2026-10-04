import { getCurrentWindow } from "@tauri-apps/api/window";
import { hasBackend } from "./backend";

/** Window controls for the custom title bar. No-ops are never exposed: callers check `available`. */
export const windowControls = {
  available: () => hasBackend(),
  minimize: () => getCurrentWindow().minimize(),
  toggleMaximize: () => getCurrentWindow().toggleMaximize(),
  close: () => getCurrentWindow().close(),
  isMaximized: () => getCurrentWindow().isMaximized(),
  onResized: (cb: () => void) => getCurrentWindow().onResized(cb),
};
