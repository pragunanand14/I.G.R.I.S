import { ClipboardPaste, Send } from "lucide-react";
import { useState } from "react";
import { useDeviceStore } from "@/stores/deviceStore";

/**
 * Send a link or a note to another of your devices. A single web link is sent
 * as a link (it gets an Open button there); anything else as text.
 */
export function SendToDevice({ phone = false }: { phone?: boolean }) {
  const overview = useDeviceStore((s) => s.overview);
  const handoff = useDeviceStore((s) => s.handoff);
  const busy = useDeviceStore((s) => s.busy);
  const [to, setTo] = useState<string>("");
  const [text, setText] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const devices = overview?.devices.filter((d) => d.trusted && !d.revokedAt) ?? [];
  const target = devices.find((d) => d.deviceId === to) ?? devices[0];
  if (!target) return null;

  const send = async () => {
    if (!text.trim()) return;
    const r = await handoff(target.deviceId, text);
    if (r === null) return;
    setText("");
    setNote(r ? `Sent to ${target.name}.` : `${target.name} isn't reachable right now — it'll get it when the connection is back.`);
  };
  const paste = () =>
    void navigator.clipboard
      ?.readText()
      .then((t) => t && setText(t.slice(0, 2000)))
      .catch(() => setNote("Couldn't read the clipboard — paste it into the box instead."));

  const field = phone ? "m-input py-3" : "w-full rounded-xl bg-surface-strong px-3 py-2 text-sm text-fg placeholder:text-faint focus:outline-none";
  return (
    <div className="space-y-2">
      {devices.length > 1 && (
        <select aria-label="Send to" value={target.deviceId} onChange={(e) => setTo(e.target.value)} className={field}>
          {devices.map((d) => (
            <option key={d.deviceId} value={d.deviceId}>
              {d.name}
            </option>
          ))}
        </select>
      )}
      <textarea
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          setNote(null);
        }}
        rows={phone ? 3 : 2}
        maxLength={2000}
        placeholder={`A link or a note for ${target.name}`}
        aria-label={`Send to ${target.name}`}
        className={`${field} resize-none`}
      />
      <div className="flex gap-2">
        <button type="button" onClick={paste} className={phone ? "m-button flex-1" : "flex items-center gap-1.5 rounded-full bg-surface-strong px-3.5 py-1.5 text-xs font-medium text-fg hover:bg-surface-hover"}>
          <ClipboardPaste className="size-4" /> Paste
        </button>
        <button
          type="button"
          disabled={busy || !text.trim()}
          onClick={() => void send()}
          className={phone ? "m-button m-button-primary flex-1" : "flex items-center gap-1.5 rounded-full bg-accent px-3.5 py-1.5 text-xs font-semibold text-bg hover:opacity-90 disabled:opacity-40"}
        >
          <Send className="size-4" /> Send to {target.name}
        </button>
      </div>
      {note && <p className={phone ? "m-muted px-1 text-sm" : "text-xs text-muted"}>{note}</p>}
    </div>
  );
}
