import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { useAppStore } from "@/stores/appStore";
import { useChatStore } from "@/stores/chatStore";
import type { AuditEntry } from "@/types/tools";
import { outcome } from "./activity";
import { whenText } from "./format";
import { useSystemStore } from "@/stores/systemStore";
import { HomeScreen } from "./screens/HomeScreen";
import { MoreScreen } from "./screens/MoreScreen";
import { useIntro } from "./motion";

const ai = (ready: boolean, problem: string | null = null) => ({ provider: "anthropic", configuredModel: null, ready, problem, effectiveModel: ready ? "m" : null, effort: "medium" as const });

function home() {
  return render(
    <MemoryRouter>
      <HomeScreen />
    </MemoryRouter>,
  );
}

describe("phone Home screen", () => {
  beforeEach(() => useIntro.setState({ state: "done" }));
  afterEach(() => {
    useAppStore.setState({ backend: "connecting", backendError: null });
    useChatStore.setState({ aiStatus: null, streaming: null });
  });

  it("says plainly when IGRIS isn't running and offers nothing it can't do", () => {
    useAppStore.setState({ backend: "unavailable", backendError: "Running outside the app." });
    const { container } = home();
    expect(screen.getByText("IGRIS isn't running")).toBeInTheDocument();
    expect(screen.getByLabelText("Ask IGRIS")).toBeDisabled();
    expect(container.querySelector(".m-mic")).toBeDisabled();
  });

  it("explains a missing AI provider in the backend's own words and links to setup", () => {
    useAppStore.setState({ backend: "ready" });
    useChatStore.setState({ aiStatus: ai(false, "No AI provider configured.") });
    home();
    expect(screen.getByText("Needs an AI provider")).toBeInTheDocument();
    expect(screen.getByText("No AI provider configured.")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /How to set it up/ })).toHaveAttribute("href", "/settings/ai");
  });

  it("is ready to help when the AI is connected", () => {
    useAppStore.setState({ backend: "ready" });
    useChatStore.setState({ aiStatus: ai(true) });
    home();
    expect(screen.getByText("Ready to help")).toBeInTheDocument();
    expect(screen.getByLabelText("Ask IGRIS")).toBeEnabled();
  });

  it("is the orb, one bar and three ways to everything else", () => {
    useAppStore.setState({ backend: "ready" });
    useChatStore.setState({ aiStatus: ai(true) });
    const { container } = home();
    expect(container.querySelector("[data-orb='home'] .ai-core")).not.toBeNull();
    expect(screen.getByRole("link", { name: "Your chats" })).toHaveAttribute("href", "/chats");
    expect(screen.getByRole("link", { name: "Reminders and to-dos" })).toHaveAttribute("href", "/today");
    expect(screen.getByRole("link", { name: "Settings" })).toHaveAttribute("href", "/settings");
  });

  it("turns the mic into send while there's text", () => {
    useAppStore.setState({ backend: "ready" });
    useChatStore.setState({ aiStatus: ai(true) });
    home();
    expect(screen.queryByRole("button", { name: "Send" })).toBeNull();
    fireEvent.change(screen.getByLabelText("Ask IGRIS"), { target: { value: "hello" } });
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
    fireEvent.change(screen.getByLabelText("Ask IGRIS"), { target: { value: "" } });
    expect(screen.queryByRole("button", { name: "Send" })).toBeNull();
  });
});

describe("phone More screen", () => {
  afterEach(() => useAppStore.setState({ backend: "connecting" }));

  it("lists every area and shows unknown phone values as unknown", () => {
    useAppStore.setState({ backend: "ready" });
    useSystemStore.setState({ snapshot: null });
    render(
      <MemoryRouter>
        <MoreScreen />
      </MemoryRouter>,
    );
    expect(screen.getByRole("link", { name: /Phone and computer/ })).toHaveAttribute("href", "/settings/devices");
    expect(screen.getByRole("link", { name: /AI and voice/ })).toHaveAttribute("href", "/settings/ai");
    expect(screen.getByRole("link", { name: /Appearance/ })).toHaveAttribute("href", "/settings/appearance");
    expect(screen.getByRole("link", { name: /What IGRIS remembers/ })).toHaveAttribute("href", "/settings/memory");
    expect(screen.getByRole("link", { name: /What IGRIS has done/ })).toHaveAttribute("href", "/settings/activity");
    expect(screen.getByRole("switch", { name: "Ask before every action" })).toBeInTheDocument();
    expect(screen.getAllByText("Unknown").length).toBeGreaterThanOrEqual(2);
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
