// Minimal typing for the (prefixed) Web Speech recognition API.
interface RecognitionResultEvent {
  resultIndex: number;
  results: ArrayLike<ArrayLike<{ transcript: string }> & { isFinal: boolean }>;
}
interface Recognition {
  lang: string;
  continuous: boolean;
  interimResults: boolean;
  onresult: ((e: RecognitionResultEvent) => void) | null;
  onerror: ((e: { error: string }) => void) | null;
  onend: (() => void) | null;
  start(): void;
  stop(): void;
  abort(): void;
}
type RecognitionCtor = new () => Recognition;

function ctor(): RecognitionCtor | null {
  const w = globalThis as unknown as { SpeechRecognition?: RecognitionCtor; webkitSpeechRecognition?: RecognitionCtor };
  return w.SpeechRecognition ?? w.webkitSpeechRecognition ?? null;
}

export function browserRecognitionAvailable(): boolean {
  return ctor() !== null;
}

function message(code: string): string {
  switch (code) {
    case "not-allowed":
    case "service-not-allowed":
      return "Speech recognition permission was denied.";
    case "network":
      // WebView2 often can't reach Edge's speech service even when online.
      return "The built-in speech recognition isn't available in this app. Set STT_PROVIDER=gemini (uses your Gemini key) or openai in .env, then reload.";
    case "no-speech":
      return "I didn't hear anything.";
    case "audio-capture":
      return "No microphone was found.";
    default:
      return `Speech recognition failed (${code}).`;
  }
}

/** Recognise one utterance with the webview's built-in recogniser. */
export function recognizeOnce(lang = navigator.language || "en-US"): { result: Promise<string>; cancel: () => void } {
  const C = ctor();
  if (!C) return { result: Promise.reject(new Error("Speech recognition isn't available in this webview.")), cancel: () => undefined };
  const r = new C();
  r.lang = lang;
  r.continuous = false;
  r.interimResults = false;
  let text = "";
  let cancelled = false;
  const result = new Promise<string>((resolve, reject) => {
    r.onresult = (e) => {
      for (let i = e.resultIndex; i < e.results.length; i++) text += e.results[i]![0]!.transcript;
    };
    r.onerror = (e) => (e.error === "no-speech" || e.error === "aborted" ? undefined : reject(new Error(message(e.error))));
    r.onend = () => resolve(cancelled ? "" : text.trim());
  });
  r.start();
  return {
    result,
    cancel: () => {
      cancelled = true;
      r.abort();
    },
  };
}

/** Continuous recognition used for the wake word. Restarts itself until stopped. */
export function listenContinuously(onFinal: (transcript: string) => void, onError: (msg: string) => void): () => void {
  const C = ctor();
  if (!C) {
    onError("Wake word needs speech recognition, which this webview doesn't provide.");
    return () => undefined;
  }
  let stopped = false;
  let r: Recognition | null = null;
  const begin = () => {
    if (stopped) return;
    r = new C();
    r.lang = navigator.language || "en-US";
    r.continuous = true;
    r.interimResults = false;
    r.onresult = (e) => {
      for (let i = e.resultIndex; i < e.results.length; i++) if (e.results[i]!.isFinal) onFinal(e.results[i]![0]!.transcript);
    };
    r.onerror = (e) => {
      if (e.error === "not-allowed" || e.error === "service-not-allowed" || e.error === "audio-capture") {
        stopped = true;
        onError(message(e.error));
      }
    };
    r.onend = () => setTimeout(begin, 300);
    r.start();
  };
  begin();
  return () => {
    stopped = true;
    r?.abort();
  };
}
