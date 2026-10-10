import type { ImportResult } from "@/types/app";

/** What an imported settings file changed, in plain words (setting names only, never values). */
export function importedMessage(r: ImportResult): { ok: boolean; text: string } {
  const n = r.settings.keys.length;
  const head = `Imported ${n} setting${n === 1 ? "" : "s"}.`;
  const status = r.ai.ready ? " IGRIS is ready." : ` ${r.ai.problem ?? "The AI still isn't set up."}`;
  const unknown = r.settings.unknown.length ? ` Not used (check the spelling): ${r.settings.unknown.join(", ")}.` : "";
  return { ok: r.ai.ready && !unknown, text: head + status + unknown };
}
