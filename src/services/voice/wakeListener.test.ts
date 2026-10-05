import { SEGMENTER_DEFAULTS, SpeechSegmenter } from "./wakeListener";
import { parseWakeCommand, parseYesNo } from "./wakeWord";

const FRAME_MS = 64;
const frame = (amp: number) => new Float32Array(1024).fill(amp);

function feed(seg: SpeechSegmenter, pattern: [number, number][]): Float32Array[] {
  const out: Float32Array[] = [];
  for (const [amp, ms] of pattern) {
    for (let t = 0; t < ms; t += FRAME_MS) {
      const p = seg.push(frame(amp));
      if (p) out.push(p);
    }
  }
  return out;
}

describe("SpeechSegmenter", () => {
  it("emits one phrase per utterance, with pre-roll, after trailing silence", () => {
    const seg = new SpeechSegmenter(FRAME_MS);
    const phrases = feed(seg, [
      [0.002, 2000], // quiet room
      [0.2, 900], // "IGRIS, open Chrome"
      [0.002, 1000],
    ]);
    expect(phrases).toHaveLength(1);
    const ms = (phrases[0]!.length / 1024) * FRAME_MS;
    expect(ms).toBeGreaterThan(900 + 400); // speech + some pre-roll + trailing silence
  });

  it("ignores clicks and short blips", () => {
    const seg = new SpeechSegmenter(FRAME_MS);
    expect(feed(seg, [[0.002, 1000], [0.3, 64], [0.002, 1000], [0.3, 192], [0.002, 1500]])).toHaveLength(0);
  });

  it("adapts to steady background noise (e.g. a fan)", () => {
    const seg = new SpeechSegmenter(FRAME_MS);
    feed(seg, [[0.01, 6000]]);
    expect(seg.threshold()).toBeGreaterThan(SEGMENTER_DEFAULTS.minThreshold);
    expect(feed(seg, [[0.012, 3000]])).toHaveLength(0); // the fan itself never triggers
  });

  it("caps very long phrases", () => {
    const seg = new SpeechSegmenter(FRAME_MS);
    expect(feed(seg, [[0.002, 500], [0.2, 13_000]]).length).toBeGreaterThanOrEqual(1);
  });
});

describe("wake phrases", () => {
  it("accepts the name at the start, with optional greetings", () => {
    expect(parseWakeCommand("IGRIS, open Chrome.")).toBe("open Chrome.");
    expect(parseWakeCommand("Hey Igris what's my CPU usage?")).toBe("what's my CPU usage?");
    expect(parseWakeCommand("Wake up, IGRIS.")).toBe("");
    expect(parseWakeCommand("ok eagris remind me in five minutes")).toBe("remind me in five minutes");
  });

  it("ignores the name in the middle of ordinary speech", () => {
    expect(parseWakeCommand("I told Igris to do it yesterday")).toBeNull();
    expect(parseWakeCommand("what's for dinner")).toBeNull();
  });

  it("understands spoken yes/no", () => {
    expect(parseYesNo("Yes, go ahead.")).toBe(true);
    expect(parseYesNo("No, don't.")).toBe(false);
    expect(parseYesNo("hmm what")).toBeNull();
  });
});
