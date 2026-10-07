/**
 * Which kind of device the UI runs on. Synchronous (the app shell needs it
 * before the backend answers): the Android/iOS webview identifies itself in
 * its user agent.
 */
export function isMobilePlatform(userAgent: string = typeof navigator === "undefined" ? "" : navigator.userAgent): boolean {
  return /Android|iPhone|iPad|iPod/i.test(userAgent);
}

/** Pages that only make sense on a computer (code projects live on the desktop). */
export const DESKTOP_ONLY_PATHS = ["/projects"];
