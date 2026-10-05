export interface AppInfo {
  name: string;
  version: string;
  platform: string;
  arch: string;
  debug: boolean;
  dataDir: string;
  configDir: string;
  logDir: string;
  dbPath: string;
  schemaVersion: number;
}

/** Redacted configuration — secrets are never sent to the UI, only whether they exist. */
export interface PublicConfig {
  aiProvider: string | null;
  aiModel: string | null;
  aiBaseUrl: string | null;
  aiKeyConfigured: boolean;
  searchConfigured: boolean;
  /** brave | tavily | anthropic (built-in) | null */
  webSearch: string | null;
  ttsProvider: string | null;
  sttProvider: string | null;
  envFiles: string[];
}

export interface ReloadResult {
  config: PublicConfig;
  ai: import("./ai").AiStatus;
}
