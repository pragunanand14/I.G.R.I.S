import { getCurrentWindow } from "@tauri-apps/api/window";
import { hasBackend } from "./backend";
import { isMobilePlatform } from "./platform";

/**
 * Window controls for the custom title bar. No-ops are never exposed: callers check `available`.
 * Phones have no window to minimise, maximise or close.
 */
export const windowControls = {
  available: () => hasBackend() && !isMobilePlatform(),
  minimize: () => getCurrentWindow().minimize(),
  toggleMaximize: () => getCurrentWindow().toggleMaximize(),
  close: () => getCurrentWindow().close(),
  isMaximized: () => getCurrentWindow().isMaximized(),
  onResized: (cb: () => void) => getCurrentWindow().onResized(cb),
};
