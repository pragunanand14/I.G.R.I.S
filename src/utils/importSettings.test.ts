import { describe, expect, it } from "vitest";
import type { ImportResult } from "@/types/app";
import { importedMessage } from "./importSettings";

const result = (ready: boolean, keys: string[], unknown: string[] = []) =>
  ({ settings: { keys, unknown }, config: {}, ai: { ready, problem: ready ? null : "AI_API_KEY is missing." } }) as unknown as ImportResult;

describe("importedMessage", () => {
  it("says what was imported and that IGRIS is ready", () => {
    expect(importedMessage(result(true, ["AI_PROVIDER", "AI_API_KEY"]))).toEqual({ ok: true, text: "Imported 2 settings. IGRIS is ready." });
  });
  it("passes on what's still missing", () => {
    expect(importedMessage(result(false, ["AI_PROVIDER"])).text).toBe("Imported 1 setting. AI_API_KEY is missing.");
  });
  it("points out names IGRIS doesn't use", () => {
    const m = importedMessage(result(true, ["AI_API_KEY"], ["AI_MODLE"]));
    expect(m.ok).toBe(false);
    expect(m.text).toContain("Not used (check the spelling): AI_MODLE.");
  });
});
