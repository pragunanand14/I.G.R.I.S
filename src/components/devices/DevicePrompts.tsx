import { Monitor, ShieldAlert, Smartphone, X } from "lucide-react";
import { useEffect } from "react";
import { useDeviceStore } from "@/stores/deviceStore";
import { platformLabel } from "./deviceText";

/**
 * Cross-device prompts on top of every screen: "Allow this device to join?",
 * approvals another device asks for, approvals for tasks another device sent
 * here, and short notices ("Done — completed on My PC"). `phone` uses the
 * phone UI's styles.
 */
export function DevicePrompts({ enabled, phone = false }: { enabled: boolean; phone?: boolean }) {
  const connect = useDeviceStore((s) => s.connect);
  useEffect(() => (enabled ? connect() : undefined), [enabled, connect]);
  const { pairingRequests, overview, localApprovals, notices, confirmPairing, answer, answerLocal, dismissNotice } = useDeviceStore();
  const approvals = overview?.approvals ?? [];

  // Notices fade on their own.
  useEffect(() => {
    const first = notices[0];
    if (!first) return;
    const t = setTimeout(() => dismissNotice(first.id), 8000);
    return () => clearTimeout(t);
  }, [notices, dismissNotice]);

  if (!enabled || (pairingRequests.length === 0 && approvals.length === 0 && localApprovals.length === 0 && notices.length === 0)) return null;

  const card = phone ? "m-card m-float p-4" : "anim-rise rounded-2xl border border-line-strong bg-elevated p-3.5 shadow-[var(--shadow)]";
  const primary = phone ? "m-button m-button-primary flex-1" : "rounded-md bg-accent px-3 py-1 text-xs font-semibold text-bg";
  const secondary = phone ? "m-button flex-1" : "rounded-md border border-line px-2.5 py-1 text-xs text-muted hover:text-fg";
  const muted = phone ? "m-muted text-sm" : "text-xs text-muted";
  const title = phone ? "font-semibold" : "text-sm font-medium text-fg";

  return (
    <div
      className={phone ? "fixed inset-x-0 top-0 z-50 space-y-2 px-4 pt-[calc(env(safe-area-inset-top)+12px)]" : "pointer-events-none fixed top-12 right-4 z-50 flex w-96 flex-col gap-2"}
      aria-live="assertive"
    >
      {pairingRequests.map((p) => (
        <div key={p.pairingId} role="alertdialog" aria-label="Allow device" className={`pointer-events-auto ${card}`}>
          <div className="flex items-start gap-3">
            {p.platform === "android" ? <Smartphone className="size-5 shrink-0" /> : <Monitor className="size-5 shrink-0" />}
            <div className="min-w-0 flex-1">
              <div className={title}>
                Allow "{p.deviceName}" ({platformLabel(p.platform)}) to join your IGRIS?
              </div>
              <p className={`mt-1 ${muted}`}>
                It entered the code shown here. Once allowed it can send tasks to your devices and approve their actions, and memories will sync. You can
                remove it any time.
              </p>
            </div>
          </div>
          <div className="mt-3 flex gap-2">
            <button type="button" className={secondary} onClick={() => void confirmPairing(p.pairingId, false)}>
              Don't allow
            </button>
            <button type="button" className={primary} onClick={() => void confirmPairing(p.pairingId, true)}>
              Allow
            </button>
          </div>
        </div>
      ))}

      {approvals.map((a) => (
        <div key={a.request.callId} role="alertdialog" aria-label="Approve action" className={`pointer-events-auto ${card}`}>
          <div className="flex items-start gap-3">
            <ShieldAlert className="size-5 shrink-0" />
            <div className="min-w-0 flex-1">
              <div className={title}>{a.deviceName} needs your OK</div>
              <p className={`mt-1 break-words ${phone ? "" : "text-sm text-fg"}`}>{a.request.description}</p>
              <p className={`mt-1 ${muted}`}>
                {a.request.title} · {a.request.permission} · runs on {a.deviceName}
              </p>
            </div>
          </div>
          <div className="mt-3 flex gap-2">
            <button type="button" className={secondary} onClick={() => void answer(a.request.callId, false)}>
              Deny
            </button>
            <button type="button" className={primary} onClick={() => void answer(a.request.callId, true)}>
              Approve
            </button>
          </div>
        </div>
      ))}

      {localApprovals.map((a) => (
        <div key={a.callId} role="alertdialog" aria-label="Approve action" className={`pointer-events-auto ${card}`}>
          <div className="flex items-start gap-3">
            <ShieldAlert className="size-5 shrink-0" />
            <div className="min-w-0 flex-1">
              <div className={title}>Task from {a.deviceName} needs approval</div>
              <p className={`mt-1 break-words ${phone ? "" : "text-sm text-fg"}`}>{a.description}</p>
              <p className={`mt-1 ${muted}`}>{a.title} · runs here. You can also answer on {a.deviceName}.</p>
            </div>
          </div>
          <div className="mt-3 flex gap-2">
            <button type="button" className={secondary} onClick={() => void answerLocal(a.callId, false)}>
              Deny
            </button>
            <button type="button" className={primary} onClick={() => void answerLocal(a.callId, true)}>
              Approve
            </button>
          </div>
        </div>
      ))}

      {notices.map((n) => (
        <div key={n.id} role="status" className={`pointer-events-auto ${card}`}>
          <div className="flex items-start gap-2">
            <div className="min-w-0 flex-1">
              <div className={title}>{n.title}</div>
              {n.body && <p className={`mt-0.5 break-words ${muted}`}>{n.body}</p>}
            </div>
            <button type="button" aria-label="Dismiss" onClick={() => dismissNotice(n.id)} className={phone ? "m-faint p-1" : "rounded-md p-1 text-faint hover:text-fg"}>
              <X className="size-4" />
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}
