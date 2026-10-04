import type { ToolActivity } from "@/types/tools";

export type Part = { kind: "text"; text: string } | { kind: "tools"; activities: ToolActivity[] };

/** Split response text at the points where tools were called (offsets are code points). */
export function interleave(text: string, activities: ToolActivity[]): Part[] {
  const chars = Array.from(text);
  const parts: Part[] = [];
  let cursor = 0;
  const groups = new Map<number, ToolActivity[]>();
  for (const a of activities) {
    const at = Math.min(Math.max(a.textOffset ?? 0, 0), chars.length);
    groups.set(at, [...(groups.get(at) ?? []), a]);
  }
  for (const at of [...groups.keys()].sort((x, y) => x - y)) {
    const seg = chars.slice(cursor, at).join("").trim();
    if (seg) parts.push({ kind: "text", text: seg });
    parts.push({ kind: "tools", activities: groups.get(at)! });
    cursor = at;
  }
  const rest = chars.slice(cursor).join("").trim();
  if (rest) parts.push({ kind: "text", text: rest });
  return parts;
}

