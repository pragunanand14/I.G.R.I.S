export interface VoiceBackendStatus {
  /** browser | gemini | openai | local */
  mode: string;
  problem: string | null;
}

export interface VoiceStatus {
  stt: VoiceBackendStatus;
  tts: VoiceBackendStatus;
}

export type VoicePhase = "idle" | "listening" | "transcribing" | "speaking";

export const VOICE_HOTKEY_LABEL = "Ctrl+Shift+Space";
