import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { api } from "@/services/api";
import type { Attachment } from "@/types/attachments";
import { Composer } from "./Composer";

describe("Composer", () => {
  it("sends on Enter and inserts newlines with Shift+Enter", async () => {
    const onSend = vi.fn().mockResolvedValue(true);
    render(<Composer onSend={onSend} onStop={() => undefined} streaming={false} disabled={false} />);
    const box = screen.getByLabelText("Message IGRIS");
    await userEvent.type(box, "line one{Shift>}{Enter}{/Shift}line two{Enter}");
    expect(onSend).toHaveBeenCalledWith("line one\nline two", []);
    expect(box).toHaveValue("");
  });

  it("restores the draft when sending is rejected", async () => {
    const onSend = vi.fn().mockResolvedValue(false);
    render(<Composer onSend={onSend} onStop={() => undefined} streaming={false} disabled={false} />);
    const box = screen.getByLabelText("Message IGRIS");
    await userEvent.type(box, "keep me{Enter}");
    expect(box).toHaveValue("keep me");
  });

  it("shows a stop button while streaming", async () => {
    const onStop = vi.fn();
    render(<Composer onSend={vi.fn()} onStop={onStop} streaming disabled={false} />);
    await userEvent.click(screen.getByRole("button", { name: "Stop responding" }));
    expect(onStop).toHaveBeenCalled();
  });

  it("explains why it is disabled", () => {
    render(<Composer onSend={vi.fn()} onStop={() => undefined} streaming={false} disabled disabledReason="Configure an AI provider." />);
    expect(screen.getByLabelText("Message IGRIS")).toHaveAttribute("placeholder", "Configure an AI provider.");
  });

  describe("attachments", () => {
    const attachment: Attachment = {
      id: "att-1", kind: "pdf", mime: "application/pdf", name: "notes.pdf", size: 1200, width: null, height: null,
      source: "upload", conversationId: null, messageId: null, createdAt: "",
    };

    it("uploads a picked file and sends its id, even without text", async () => {
      const attach = vi.spyOn(api, "attachFile").mockResolvedValue(attachment);
      const onSend = vi.fn().mockResolvedValue(true);
      const { container } = render(<Composer onSend={onSend} onStop={() => undefined} streaming={false} disabled={false} attachments />);
      const input = container.querySelector('input[type="file"]') as HTMLInputElement;
      await userEvent.upload(input, new File(["%PDF-1.4"], "notes.pdf", { type: "application/pdf" }));
      expect(attach).toHaveBeenCalled();
      expect(await screen.findByText("PDF")).toBeInTheDocument();
      await userEvent.click(screen.getByRole("button", { name: "Send" }));
      expect(onSend).toHaveBeenCalledWith("", ["att-1"]);
      expect(screen.queryByText("notes.pdf")).not.toBeInTheDocument();
    });

    it("rejects unsupported files before uploading and can remove them", async () => {
      const attach = vi.spyOn(api, "attachFile").mockResolvedValue(attachment);
      render(<Composer onSend={vi.fn()} onStop={() => undefined} streaming={false} disabled={false} attachments />);
      const box = screen.getByLabelText("Message IGRIS");
      const exe = new File(["MZ"], "setup.exe", { type: "application/x-msdownload" });
      box.focus();
      await userEvent.paste({ files: [exe], getData: () => "", types: ["Files"], items: [] } as unknown as DataTransfer);
      expect(await screen.findByText(/Only images/)).toBeInTheDocument();
      expect(attach).not.toHaveBeenCalled();
      expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
      await userEvent.click(screen.getByRole("button", { name: "Remove setup.exe" }));
      expect(screen.queryByText(/Only images/)).not.toBeInTheDocument();
    });

    it("hides the attach button unless enabled", () => {
      render(<Composer onSend={vi.fn()} onStop={() => undefined} streaming={false} disabled={false} />);
      expect(screen.queryByLabelText("Attach images or PDFs")).not.toBeInTheDocument();
    });
  });
});
