import { useEffect, useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import type { ParsedTime } from "@/types/productivity";

export type TimePreview = { state: "empty" } | { state: "ok"; parsed: ParsedTime } | { state: "error"; message: string };

/** Live, debounced resolution of natural-language time input ("tomorrow at 5pm"). */
export function useTimePreview(text: string, delay = 300): TimePreview {
  const [preview, setPreview] = useState<TimePreview>({ state: "empty" });
  const blank = !text.trim();
  useEffect(() => {
    if (blank) return;
    let live = true;
    const t = setTimeout(() => {
      api
        .parseTime(text)
        .then((parsed) => live && setPreview({ state: "ok", parsed }))
        .catch((err) => live && setPreview({ state: "error", message: BackendError.from(err).message }));
    }, delay);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [text, blank, delay]);
  // While typing, the previous result stays until the new one resolves; the backend re-validates on submit.
  return blank ? { state: "empty" } : preview;
}
