import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { api } from "@/services/api";
import { GoogleAccount } from "./GoogleAccount";

describe("GoogleAccount", () => {
  afterEach(() => vi.restoreAllMocks());

  it("guides through setup when the OAuth client isn't configured", async () => {
    vi.spyOn(api, "googleStatus").mockResolvedValue({ configured: false, connected: false, email: null });
    render(<GoogleAccount />);
    expect(await screen.findByText("Needs a one-time setup")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Google setup guide" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Connect Google" })).toBeNull();
  });

  it("connects and disconnects, showing only the email", async () => {
    vi.spyOn(api, "googleStatus").mockResolvedValue({ configured: true, connected: false, email: null });
    vi.spyOn(api, "googleConnect").mockResolvedValue({ configured: true, connected: true, email: "ada@example.com" });
    const off = vi.spyOn(api, "googleDisconnect").mockResolvedValue({ configured: true, connected: false, email: null });
    render(<GoogleAccount />);
    await userEvent.click(await screen.findByRole("button", { name: "Connect Google" }));
    expect(await screen.findByText("Connected as ada@example.com")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Disconnect" }));
    expect(off).toHaveBeenCalled();
    expect(await screen.findByText("Not connected")).toBeInTheDocument();
  });
});
