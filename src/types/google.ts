/** The user's Google account as the UI sees it: never any token. */
export interface GoogleStatus {
  /** Connecting can start: the phone's own sign-in, or a Google client set up on this computer. */
  configured: boolean;
  /** Signing in uses the phone's Google account picker. */
  native?: boolean;
  connected: boolean;
  email: string | null;
}

/** What Google needs for IGRIS's "Android" sign-in entry (the setup guide). */
export interface GoogleSetupInfo {
  packageName: string;
  sha1: string;
}
