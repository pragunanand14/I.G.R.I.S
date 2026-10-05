import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink } from "lucide-react";
import { useState } from "react";
import { hasBackend } from "@/services/backend";

function host(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

/** Source links under a web tool result; open in the system browser. */
export function SourceLinks({ sources }: { sources: { title: string; url: string }[] }) {
  const [all, setAll] = useState(false);
  const shown = all ? sources : sources.slice(0, 4);
  return (
    <ul className="mt-1.5 flex flex-wrap gap-1.5 pl-5.5" aria-label="Sources">
      {shown.map((s) => (
        <li key={s.url}>
          <a
            href={s.url}
            title={s.url}
            onClick={(e) => {
              e.preventDefault();
              if (/^https?:\/\//i.test(s.url) && hasBackend()) void openUrl(s.url);
            }}
            className="flex max-w-56 items-center gap-1 rounded-md border border-line px-1.5 py-0.5 text-[10px] text-muted hover:border-accent hover:text-accent"
          >
            <ExternalLink className="size-2.5 shrink-0" />
            <span className="truncate">{s.title || host(s.url)}</span>
            <span className="shrink-0 text-faint">· {host(s.url)}</span>
          </a>
        </li>
      ))}
      {sources.length > 4 && !all && (
        <li>
          <button type="button" onClick={() => setAll(true)} className="rounded-md px-1.5 py-0.5 text-[10px] text-faint hover:text-fg">
            +{sources.length - 4} more
          </button>
        </li>
      )}
    </ul>
  );
}
