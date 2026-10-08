import type { StreamingState } from "@/stores/chatStore";
import { streamingLabel } from "./streamingLabel";

const base: StreamingState = { requestId: "r", conversationId: "c", text: "", phase: "waiting", model: "m", activities: [], startedAt: 0 };

describe("what the chat says while IGRIS waits", () => {
  it("names a provider rate limit and counts down instead of an unexplained 'Thinking…'", () => {
    const label = streamingLabel({ ...base, retryAt: 23_000, rateLimited: true }, 1_000);
    expect(label).toBe("The AI service is limiting requests (too many in a short time) — trying again in 22 s");
  });

  it("says when the service couldn't be reached", () => {
    expect(streamingLabel({ ...base, retryAt: 5_000, rateLimited: false }, 1_000)).toMatch(/Couldn't reach the AI service — trying again in 4 s/);
  });

  it("goes back to normal once the wait is over", () => {
    expect(streamingLabel({ ...base, retryAt: 5_000, rateLimited: true }, 6_000)).toMatch(/^Thinking…/);
  });
});
