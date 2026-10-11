import { FolderInput, Undo2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Panel } from "@/components/ui/Panel";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import type { TidyStatus } from "@/types/tools";

/** Sort the Downloads folder into Documents, Images, … — now or by itself — with undo. */
export function TidyDownloads() {
  const [s, setS] = useState<TidyStatus | null>(null);
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null);
  const fail = (err: unknown) =>
    setNote({ ok: false, text: BackendError.from(err).message });

  const load = useCallback(() => {
    api
      .tidyStatus()
      .then(setS, (err) =>
        setNote({ ok: false, text: BackendError.from(err).message }),
      );
  }, []);
  useEffect(load, [load]);

  if (!s?.dir) return null;
  const chip =
    "flex items-center gap-1 rounded-full bg-surface-strong px-3 py-1.5 text-xs font-medium text-fg hover:bg-surface-hover disabled:opacity-40";
  return (
    <Panel title="Tidy Downloads">
      <p className="mb-3 text-xs text-muted">
        Moves loose files in <span className="text-fg">{s.dir}</span> into
        Documents, Images, Videos, Music, Archives and Installers folders.
        Nothing is renamed or overwritten, downloads still in progress stay, and
        you can undo the last tidy.
      </p>
      <p className="mb-3 text-sm text-fg">
        {s.pending ? `Ready to sort ${s.pending}.` : "Downloads is tidy."}
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          className={chip}
          disabled={!s.pending}
          onClick={() =>
            void api.tidyNow().then((r) => {
              setNote({
                ok: r.errors.length === 0,
                text: r.moved
                  ? `Moved ${r.summary}.${r.errors.length ? ` ${r.errors.length} couldn't move (open?).` : ""}`
                  : "Nothing to move.",
              });
              load();
            }, fail)
          }
        >
          <FolderInput className="size-3.5" /> Tidy now
        </button>
        <button
          type="button"
          className={chip}
          disabled={s.last === 0}
          onClick={() =>
            void api.tidyUndo().then((n) => {
              setNote({
                ok: true,
                text: `Put ${n} file${n === 1 ? "" : "s"} back.`,
              });
              load();
            }, fail)
          }
        >
          <Undo2 className="size-3.5" /> Undo last tidy
        </button>
        <label className="ml-1 flex items-center gap-2 text-sm text-fg">
          <input
            type="checkbox"
            role="switch"
            checked={s.auto}
            onChange={(e) =>
              void api.tidySetAuto(e.target.checked).then(load, fail)
            }
          />
          Tidy by itself (every 10 minutes while IGRIS runs)
        </label>
      </div>
      {note && (
        <p className={`mt-2 text-xs ${note.ok ? "text-muted" : "text-danger"}`}>
          {note.text}
        </p>
      )}
    </Panel>
  );
}
