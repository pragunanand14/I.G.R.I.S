import { useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";

/**
 * Android blocks accessibility and notification access for apps installed from a file ("restricted
 * setting"). The way out is in IGRIS's App info → ⋮ → Allow restricted settings — but Android only shows
 * that option after the user has tried to switch the setting on once and been blocked.
 */
export function RestrictedHelp({ setting }: { setting: string }) {
  const [error, setError] = useState<string | null>(null);
  return (
    <li>
      If Android says <b>“Restricted setting”</b> (it does for apps installed
      from a file):
      <ol className="mt-1 list-[lower-alpha] space-y-1 pl-5">
        <li>
          Try switching on <b>{setting}</b> first and let Android block it — the
          unlock option only appears after that.
        </li>
        <li>
          <button
            type="button"
            className="m-link"
            onClick={() => {
              setError(null);
              api
                .phoneOpenSettings("app_info")
                .catch((err) => setError(BackendError.from(err).message));
            }}
          >
            Open IGRIS app info
          </button>{" "}
          → <b>⋮</b> (top right) → <b>Allow restricted settings</b> → confirm
          with your PIN or fingerprint.
        </li>
        <li>Come back and switch it on again.</li>
      </ol>
      <p className="mt-1">
        No ⋮ option and nothing was blocked? Then it isn&apos;t restricted on
        your phone — just switch it on.
      </p>
      {error && (
        <p className="mt-1 text-sm" style={{ color: "var(--m-bad)" }}>
          {error}
        </p>
      )}
    </li>
  );
}
