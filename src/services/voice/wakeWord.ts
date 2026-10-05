// Speech recognisers often mishear "IGRIS"; accept common variants.
const NAME = String.raw`(?:igris|i\.?\s?g\.?\s?r\.?\s?i\.?\s?s|eagris|egris|iggris|igress|igres|ig\s?ris|eye\s?gris|eager[ i]?s|e\s?grace)`;
const WAKE = new RegExp(String.raw`\b${NAME}\b[,.!?\s]*`, "i");
// Always-on mode: the name must open the phrase, optionally after a greeting.
const LEADING = new RegExp(String.raw`^\W*(?:(?:hey|hi|hello|ok|okay|yo|wake\s*up)\W+)*${NAME}\b[,.!?\s]*`, "i");

/** If `transcript` contains the wake word, returns the command spoken after it ("" if none). */
export function parseWakeWord(transcript: string): string | null {
  const m = WAKE.exec(transcript);
  if (!m) return null;
  return transcript.slice(m.index + m[0].length).trim();
}

/**
 * Stricter, for always-on listening: only phrases that *start* with the name
 * ("IGRIS, open Chrome", "Hey IGRIS…", "Wake up IGRIS") count, so the name in
 * ordinary conversation ("I told Igris yesterday") doesn't trigger it.
 */
export function parseWakeCommand(transcript: string): string | null {
  const m = LEADING.exec(transcript.trim());
  if (!m) return null;
  return transcript.trim().slice(m[0].length).replace(/^[\s,.!?]+/, "").trim();
}

const YES = /\b(yes|yeah|yep|yup|sure|allow|allowed|okay|ok|go ahead|do it|confirm|approve|affirmative)\b/i;
const NO = /\b(no|nope|nah|don'?t|do not|deny|denied|cancel|stop|negative)\b/i;

/** Spoken answer to an approval prompt: true / false, or null if unclear. */
export function parseYesNo(transcript: string): boolean | null {
  const yes = YES.test(transcript);
  const no = NO.test(transcript);
  if (yes === no) return null;
  return yes;
}

/** Spoken operator controls ("stop", "pause", "continue") — after the wake word has been stripped. */
export function parseOperatorCommand(command: string): "stop" | "pause" | "resume" | null {
  const c = command
    .toLowerCase()
    .replace(/[^a-z\s]/g, " ")
    .trim()
    .replace(/\s+/g, " ");
  if (/^(stop|cancel|abort|halt|enough|emergency stop)( it| that| now| everything)?$/.test(c)) return "stop";
  if (/^(pause|wait|hold on|hold)( it| that| now| a (second|moment|sec))?$/.test(c)) return "pause";
  if (/^(resume|continue|go on|carry on|keep going|go ahead)( now)?$/.test(c)) return "resume";
  return null;
}
