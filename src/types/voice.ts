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

/** "Hey IGRIS" on the phone, and whether IGRIS is the phone's digital assistant. */
export interface WakeStatus {
  on: boolean;
  chargingOnly: boolean;
  running: boolean;
  listening: boolean;
  problem: string | null;
  /** "Display over other apps": IGRIS can open itself when it hears its name. */
  canOpenItself: boolean;
  batteryUnrestricted: boolean;
  assistant: boolean;
}
