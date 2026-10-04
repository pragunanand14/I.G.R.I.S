import type { ToolActivity } from "@/types/tools";
import { interleave } from "./interleave";

const act = (id: string, textOffset?: number): ToolActivity => ({
  id,
  tool: "calculator",
  title: "Calculator",
  permission: "safe",
  description: "Calculate",
  status: "completed",
  result: "= 42",
  durationMs: 1,
  textOffset,
});

describe("interleave", () => {
  it("places tool calls between the text written before and after them", () => {
    const parts = interleave("Let me check.\n\nIt's 42.", [act("a", 13)]);
    expect(parts.map((p) => (p.kind === "text" ? p.text : `[${p.activities.map((a) => a.id).join(",")}]`))).toEqual([
      "Let me check.",
      "[a]",
      "It's 42.",
    ]);
  });

  it("groups parallel calls and counts offsets in code points", () => {
    const parts = interleave("✓ 🚀 ok\n\ndone", [act("a", 6), act("b", 6)]);
    expect(parts[0]).toEqual({ kind: "text", text: "✓ 🚀 ok" });
    expect(parts[1]).toMatchObject({ kind: "tools" });
    expect(parts[1]!.kind === "tools" && parts[1]!.activities).toHaveLength(2);
    expect(parts[2]).toEqual({ kind: "text", text: "done" });
  });

  it("handles tools before any text and missing offsets", () => {
    expect(interleave("", [act("a")])).toEqual([{ kind: "tools", activities: [act("a")] }]);
    expect(interleave("hi", [act("a", 999)]).map((p) => p.kind)).toEqual(["text", "tools"]);
  });
});
