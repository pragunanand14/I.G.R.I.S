import { invoke, isTauri } from "@tauri-apps/api/core";

export type BackendErrorKind =
  | "validation"
  | "database"
  | "serialization"
  | "io"
  | "internal"
  | "unavailable"
  | "unknown";

/** Normalised error for every backend call. */
export class BackendError extends Error {
  readonly kind: BackendErrorKind;

  constructor(kind: BackendErrorKind, message: string) {
    super(message);
    this.name = "BackendError";
    this.kind = kind;
  }

  static from(err: unknown): BackendError {
    if (err instanceof BackendError) return err;
    if (err && typeof err === "object" && "kind" in err && "message" in err) {
      const e = err as { kind: unknown; message: unknown };
      return new BackendError(
        (typeof e.kind === "string" ? e.kind : "unknown") as BackendErrorKind,
        String(e.message),
      );
    }
    if (typeof err === "string") return new BackendError("unknown", err);
    if (err instanceof Error) return new BackendError("unknown", err.message);
    return new BackendError("unknown", "An unknown error occurred.");
  }
}

/** True when running inside the Tauri desktop shell (vs. a plain browser). */
export function hasBackend(): boolean {
  return isTauri();
}

/**
 * Typed wrapper around Tauri `invoke`. Outside the desktop shell there is no
 * backend, so calls fail explicitly instead of returning placeholder data.
 */
export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!hasBackend()) {
    throw new BackendError(
      "unavailable",
      "The IGRIS desktop backend is not available. Run the app with `npm run tauri dev`.",
    );
  }
  try {
    return await invoke<T>(command, args);
  } catch (err) {
    throw BackendError.from(err);
  }
}

/** Invoke a command whose body is raw bytes (file uploads) instead of JSON. */
export async function callRaw<T>(command: string, body: Uint8Array, headers: Record<string, string>): Promise<T> {
  if (!hasBackend()) {
    throw new BackendError("unavailable", "The IGRIS desktop backend is not available.");
  }
  try {
    return await invoke<T>(command, body, { headers });
  } catch (err) {
    throw BackendError.from(err);
  }
}
