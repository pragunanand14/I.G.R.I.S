import type { ToolActivity } from "@/types/tools";
import { interleave } from "./interleave";
import { Markdown } from "./Markdown";
import { ToolActivityList } from "./ToolActivityList";

interface Props {
  text: string;
  activities: ToolActivity[];
  onAnswer?: (callId: string, approved: boolean) => void;
  /** Show a streaming caret after the last text segment. */
  streaming?: boolean;
}

/** Assistant text with tool calls shown where they happened. */
export function AssistantBody({ text, activities, onAnswer, streaming }: Props) {
  const parts = interleave(text, activities);
  const lastText = parts.map((p) => p.kind).lastIndexOf("text");
  return (
    <div className="space-y-2">
      {parts.map((p, i) =>
        p.kind === "text" ? (
          <div key={i} className={streaming && i === lastText && i === parts.length - 1 ? "stream-caret" : undefined}>
            <Markdown text={p.text} />
          </div>
        ) : (
          <ToolActivityList key={i} activities={p.activities} onAnswer={onAnswer} />
        ),
      )}
    </div>
  );
}
