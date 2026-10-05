import { DEFAULT_SILENCE, rms, SilenceDetector } from "./silence";
import { SentenceChunker, toSpeakable } from "./speechText";
import { parseWakeWord } from "./wakeWord";

describe("toSpeakable", () => {
  it("strips markdown and replaces code and URLs", () => {
    expect(toSpeakable("## Result\n**Bold** and `code` see [docs](https://x.y) or https://a.b/c")).toBe("Result\nBold and code see docs or the link");
    expect(toSpeakable("Here:\n```ts\nconst a = 1;\n```\nDone.")).toContain("I've put the code on screen.");
    expect(toSpeakable("- one\n- two\n1. three")).toBe("one\ntwo\nthree");
  });
});

describe("SentenceChunker", () => {
  it("emits sentences as they complete", () => {
    const c = new SentenceChunker();
    expect(c.push("Opening VS")).toEqual([]);
    expect(c.push(" Code. It's ready")).toEqual(["Opening VS Code."]);
    expect(c.push("! Anything else?")).toEqual(["It's ready!"]);
    expect(c.flush()).toEqual(["Anything else?"]);
    expect(c.flush()).toEqual([]);
  });

  it("holds code blocks until closed and keeps decimals intact", () => {
    const c = new SentenceChunker();
    expect(c.push("Version 1.2 is out. Code:\n```js\nfoo(); bar(). baz")).toEqual(["Version 1.2 is out."]);
    expect(c.push("\n```\n\nThat's it.")).toEqual(["Code:\n(I've put the code on screen.)"]);
    expect(c.flush()).toEqual(["That's it."]);
  });
});

describe("parseWakeWord", () => {
  it("detects IGRIS and common mishearings", () => {
    expect(parseWakeWord("IGRIS, open VS Code")).toBe("open VS Code");
    expect(parseWakeWord("hey igris what's the time?")).toBe("what's the time?");
    expect(parseWakeWord("egress")).toBeNull();
    expect(parseWakeWord("Eagris")).toBe("");
    expect(parseWakeWord("I. G. R. I. S. hello")).toBe("hello");
    expect(parseWakeWord("the iris of the eye")).toBeNull();
  });
});

describe("SilenceDetector", () => {
  it("ends after silence following speech", () => {
    const d = new SilenceDetector({ ...DEFAULT_SILENCE, silenceMs: 1000 });
    expect(d.update(0.001, 0)).toBe("waiting");
    expect(d.update(0.2, 500)).toBe("speech");
    expect(d.update(0.001, 1200)).toBe("speech");
    expect(d.update(0.001, 1600)).toBe("done");
  });

  it("times out without speech and caps length", () => {
    const d = new SilenceDetector({ ...DEFAULT_SILENCE, noSpeechMs: 3000 });
    expect(d.update(0, 0)).toBe("waiting");
    expect(d.update(0, 3100)).toBe("timeout");
    const e = new SilenceDetector({ ...DEFAULT_SILENCE, maxMs: 2000 });
    e.update(0.5, 0);
    expect(e.update(0.5, 2100)).toBe("done");
  });

  it("computes RMS from byte samples", () => {
    expect(rms(new Uint8Array([128, 128]))).toBe(0);
    expect(rms(new Uint8Array([255, 1]))).toBeGreaterThan(0.9);
  });
});
