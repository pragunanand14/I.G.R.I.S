import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { useAppStore } from "@/stores/appStore";
import { useChatStore } from "@/stores/chatStore";
import type { AuditEntry } from "@/types/tools";
import { outcome } from "./activity";
import { whenText } from "./format";
import { HomeScreen } from "./screens/HomeScreen";

const ai = (ready: boolean, problem: string | null = null) => ({ provider: "anthropic", configuredModel: null, ready, problem, effectiveModel: ready ? "m" : null, effort: "medium" as const });

function home() {
  return render(
    <MemoryRouter>
      <HomeScreen />
    </MemoryRouter>,
  );
}

describe("phone Home screen", () => {
  afterEach(() => {
    useAppStore.setState({ backend: "connecting", backendError: null });
    useChatStore.setState({ aiStatus: null, streaming: null });
  });

  it("says plainly when IGRIS isn't running and offers nothing it can't do", () => {
    useAppStore.setState({ backend: "unavailable", backendError: "Running outside the app." });
    home();
    expect(screen.getByText("IGRIS isn't running")).toBeInTheDocument();
    expect(screen.getByLabelText("Ask IGRIS")).toBeDisabled();
    for (const chip of screen.getAllByRole("button", { name: /Remind me|battery|apps|to-do/ })) expect(chip).toBeDisabled();
  });

  it("explains a missing AI provider in the backend's own words and links to setup", () => {
    useAppStore.setState({ backend: "ready" });
    useChatStore.setState({ aiStatus: ai(false, "No AI provider configured.") });
    home();
    expect(screen.getByText("Needs an AI provider")).toBeInTheDocument();
    expect(screen.getByText("No AI provider configured.")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /How to set it up/ })).toHaveAttribute("href", "/more");
  });

  it("is ready to help when the AI is connected", () => {
    useAppStore.setState({ backend: "ready" });
    useChatStore.setState({ aiStatus: ai(true) });
    home();
    expect(screen.getByText("Ready to help")).toBeInTheDocument();
    expect(screen.getByLabelText("Ask IGRIS")).toBeEnabled();
    expect(screen.getByRole("button", { name: "What's my battery level?" })).toBeEnabled();
  });

  it("shows unknown phone values as unknown", () => {
    useAppStore.setState({ backend: "ready" });
    home();
    expect(screen.getByText("Battery unknown")).toBeInTheDocument();
  });
});

describe("phone wording", () => {
  const now = new Date(2026, 9, 7, 10, 0);
  it("describes times the way people say them", () => {
    expect(whenText(new Date(2026, 9, 7, 10, 4).toISOString(), now)).toBe("in 4 min");
    expect(whenText(new Date(2026, 9, 7, 10, 1).toISOString(), now)).toBe("in a minute");
    expect(whenText(new Date(2026, 9, 7, 18, 30).toISOString(), now)).toMatch(/^today at /);
    expect(whenText(new Date(2026, 9, 8, 9, 0).toISOString(), now)).toMatch(/^tomorrow at /);
    expect(whenText("not a date", now)).toBe("");
  });

  it("maps every executor status to an honest outcome", () => {
    const e = (status: string, approval: AuditEntry["approval"] = "auto") => ({ status, approval }) as AuditEntry;
    expect(outcome(e("completed")).text).toBe("Done");
    expect(outcome(e("completed", "approved")).text).toBe("Done, with your OK");
    expect(outcome(e("denied", "denied")).text).toBe("You said no");
    expect(outcome(e("cancelled")).text).toBe("Stopped");
    expect(outcome(e("awaiting_approval")).text).toBe("Not finished");
    expect(outcome(e("failed")).tone).toBe("bad");
    expect(outcome(e("invalid")).tone).toBe("bad");
  });
});
