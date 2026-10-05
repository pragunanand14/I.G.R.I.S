// Speech recognisers often mishear "IGRIS"; accept common variants.
const WAKE = /\b(igris|i\.?\s?g\.?\s?r\.?\s?i\.?\s?s|eagris|egris|iggris|igress|ig\s?ris|eye\s?gris)\b[,.!?\s]*/i;

/** If `transcript` contains the wake word, returns the command spoken after it ("" if none). */
export function parseWakeWord(transcript: string): string | null {
  const m = WAKE.exec(transcript);
  if (!m) return null;
  return transcript.slice(m.index + m[0].length).trim();
}
