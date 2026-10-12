import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { api } from "@/services/api";
import { GoogleAccount } from "./GoogleAccount";

describe("GoogleAccount", () => {
  afterEach(() => vi.restoreAllMocks());

  it("guides through setup when the OAuth client isn't configured", async () => {
    vi.spyOn(api, "googleStatus").mockResolvedValue({
      configured: false,
      connected: false,
      email: null,
    });
    render(<GoogleAccount />);
    expect(
      await screen.findByText("Needs a one-time setup"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Google setup guide" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Sign in with Google" }),
    ).toBeNull();
  });

  it("saves the pasted Client ID and secret, then offers sign-in", async () => {
    vi.spyOn(api, "googleStatus").mockResolvedValue({
      configured: false,
      connected: false,
      email: null,
    });
    const save = vi
      .spyOn(api, "googleSetClient")
      .mockResolvedValue({ configured: true, connected: false, email: null });
    render(<GoogleAccount />);
    await userEvent.type(
      await screen.findByLabelText("Client ID"),
      "123-abc.apps.googleusercontent.com",
    );
    await userEvent.type(
      screen.getByLabelText("Client secret"),
      "GOCSPX-secret",
    );
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(save).toHaveBeenCalledWith(
      "123-abc.apps.googleusercontent.com",
      "GOCSPX-secret",
    );
    expect(
      await screen.findByRole("button", { name: "Sign in with Google" }),
    ).toBeInTheDocument();
  });

  it("on the phone, signs in with the phone's account and shows what Google needs", async () => {
    vi.spyOn(api, "googleStatus").mockResolvedValue({
      configured: true,
      native: true,
      connected: false,
      email: null,
    });
    vi.spyOn(api, "googleSetupInfo").mockResolvedValue({
      packageName: "dev.igris.app",
      sha1: "AB:CD",
    });
    render(<GoogleAccount phone />);
    expect(
      await screen.findByRole("button", { name: "Sign in with Google" }),
    ).toBeInTheDocument();
    expect(await screen.findByText("AB:CD")).toBeInTheDocument();
    expect(screen.queryByLabelText("Client ID")).toBeNull();
  });

  it("connects and disconnects, showing only the email", async () => {
    vi.spyOn(api, "googleStatus").mockResolvedValue({
      configured: true,
      connected: false,
      email: null,
    });
    vi.spyOn(api, "googleConnect").mockResolvedValue({
      configured: true,
      connected: true,
      email: "ada@example.com",
    });
    const off = vi
      .spyOn(api, "googleDisconnect")
      .mockResolvedValue({ configured: true, connected: false, email: null });
    render(<GoogleAccount />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Sign in with Google" }),
    );
    expect(
      await screen.findByText("Connected as ada@example.com"),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Disconnect" }));
    expect(off).toHaveBeenCalled();
    expect(await screen.findByText("Not connected")).toBeInTheDocument();
  });
});
