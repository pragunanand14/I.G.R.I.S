/** The user's Google account as the UI sees it: never any token. */
export interface GoogleStatus {
  /** GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET are in IGRIS's settings file. */
  configured: boolean;
  connected: boolean;
  email: string | null;
}
