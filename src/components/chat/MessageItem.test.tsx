import { render, screen } from "@testing-library/react";
import type { Message } from "@/types/chat";
import { MessageItem } from "./MessageItem";

const base: Message = {
  id: "a1",
  conversationId: "c1",
  seq: 2,
  role: "assistant",
  content: "",
  status: "complete",
  error: null,
  provider: "anthropic",
  model: "claude-opus-5-5",
  inputTokens: null,
  outputTokens: null,
  createdAt: "2026-10-04T10:00:00Z",
  toolActivity: null,
};

const renderItem = (m: Partial<Message>, isLast = true) =>
  render(<MessageItem message={{ ...base, ...m }} isLastAssistant={isLast} busy={false} onRegenerate={() => undefined} onEdit={async () => true} />);

describe("MessageItem", () => {
  it("shows the real error and offers retry for failed turns", () => {
    renderItem({ status: "error", error: "Anthropic rejected the API key. Check AI_API_KEY in your .env file." });
    expect(screen.getByRole("alert")).toHaveTextContent("AI_API_KEY");
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("labels refusals and stopped responses honestly", () => {
    renderItem({ status: "refused", error: "The model declined this request." });
    expect(screen.getByText("The model declined this request.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  });

  it("keeps partial text of a stopped response and marks it", () => {
    renderItem({ status: "cancelled", content: "Partial answer" });
    expect(screen.getByText("Partial answer")).toBeInTheDocument();
    expect(screen.getByText("Stopped.")).toBeInTheDocument();
  });

  it("offers editing only on user messages", () => {
    renderItem({ role: "user", content: "Hi" });
    expect(screen.getByRole("button", { name: "Edit" })).toBeInTheDocument();
  });
});
