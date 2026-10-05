/** Convert Markdown to text that sounds natural when read aloud. */
export function toSpeakable(markdown: string): string {
  return (
    markdown
      // Fenced code blocks are shown, not read.
      .replace(/```[\s\S]*?(```|$)/g, " (I've put the code on screen.) ")
      .replace(/`([^`]+)`/g, "$1")
      // Images and links: keep the label.
      .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
      .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
      // Bare URLs are unpleasant to hear.
      .replace(/https?:\/\/\S+/g, "the link")
      // Headings, emphasis, list markers, tables, quotes.
      .replace(/^#{1,6}\s+/gm, "")
      .replace(/(\*\*|__)(.*?)\1/g, "$2")
      .replace(/(\*|_)(.*?)\1/g, "$2")
      .replace(/~~(.*?)~~/g, "$1")
      .replace(/^\s*[-*+]\s+/gm, "")
      .replace(/^\s*\d+\.\s+/gm, "")
      .replace(/^\s*>\s?/gm, "")
      .replace(/\|/g, " ")
      .replace(/^\s*[-:| ]{3,}\s*$/gm, "")
      .replace(/[ \t]+/g, " ")
      .replace(/ *\n */g, "\n")
      .replace(/\n{2,}/g, "\n")
      .trim()
  );
}

/**
 * Splits streamed text into sentences as soon as they are complete, so speech
 * can start before the full reply has arrived. Code blocks are held back until
 * they close so they aren't read character by character.
 */
export class SentenceChunker {
  private buffer = "";

  push(delta: string): string[] {
    this.buffer += delta;
    const out: string[] = [];
    for (;;) {
      const end = this.nextBoundary();
      if (end < 0) break;
      const sentence = this.buffer.slice(0, end);
      this.buffer = this.buffer.slice(end);
      if (toSpeakable(sentence).length >= 2) out.push(sentence);
    }
    return out.map(toSpeakable).filter((s) => s.length > 0);
  }

  /** End index of the first complete sentence outside code fences, or -1. */
  private nextBoundary(): number {
    const re = /([.!?…])(["')\]]?)(\s+)|\n\s*\n/g;
    for (let m = re.exec(this.buffer); m; m = re.exec(this.buffer)) {
      const fencesBefore = (this.buffer.slice(0, m.index).match(/```/g) ?? []).length;
      if (fencesBefore % 2 === 0) return m.index + m[0].length;
    }
    return -1;
  }

  flush(): string[] {
    const rest = toSpeakable(this.buffer);
    this.buffer = "";
    return rest ? [rest] : [];
  }
}
