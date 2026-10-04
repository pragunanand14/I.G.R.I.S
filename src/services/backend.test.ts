import { BackendError, call, hasBackend } from "./backend";

describe("backend service", () => {
  it("is unavailable outside the Tauri shell", () => {
    expect(hasBackend()).toBe(false);
  });

  it("rejects calls explicitly instead of returning placeholder data", async () => {
    await expect(call("get_settings")).rejects.toMatchObject({ kind: "unavailable" });
  });

  it("normalises serialized backend errors", () => {
    const e = BackendError.from({ kind: "validation", message: "Name too long" });
    expect(e).toBeInstanceOf(BackendError);
    expect(e.kind).toBe("validation");
    expect(e.message).toBe("Name too long");
  });

  it("normalises strings, Errors and unknown values", () => {
    expect(BackendError.from("boom").message).toBe("boom");
    expect(BackendError.from(new Error("bad")).message).toBe("bad");
    expect(BackendError.from(42).kind).toBe("unknown");
  });
});
