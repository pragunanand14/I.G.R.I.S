import { api } from "@/services/api";

/** Data URLs for stored attachments, loaded once per session. */
const cache = new Map<string, Promise<string>>();

export function attachmentUrl(id: string): Promise<string> {
  let p = cache.get(id);
  if (!p) {
    p = api.readAttachment(id).then((d) => `data:${d.mime};base64,${d.data}`);
    p.catch(() => cache.delete(id)); // allow a retry later
    cache.set(id, p);
  }
  return p;
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
