import type { Capability, LinkStatus, Platform, RemoteTask } from "@/types/devices";

export function platformLabel(p: Platform): string {
  return { windows: "Windows PC", android: "Android phone", linux: "Linux computer", macos: "Mac", other: "Device" }[p];
}

export const CAPABILITY_LABEL: Record<Capability, string> = {
  tasks: "Tasks",
  memory: "Memory",
  productivity: "Reminders & calendar",
  files: "Files",
  desktop_apps: "Desktop apps",
  terminal: "Terminal",
  browser: "Browser",
  computer_control: "Operate the computer",
  screenshots: "Screenshots",
  phone_apps: "Phone apps",
  phone_status: "Battery & network",
  web: "Web search",
};

export function linkText(s: LinkStatus | undefined): { text: string; tone: "ok" | "warn" | "bad" } {
  switch (s?.state) {
    case "connected":
      return { text: "Connected", tone: "ok" };
    case "connecting":
      return { text: "Connecting…", tone: "warn" };
    case "offline":
      return { text: s.detail ? `Can't connect — ${s.detail}` : "Can't connect", tone: "bad" };
    default:
      return { text: "Off", tone: "warn" };
  }
}

/** Where a task stands, in the words the user sees ("Sent to My PC", "Done — completed on My PC"). */
export function taskStatusText(t: RemoteTask): string {
  const who = t.peerName;
  if (t.direction === "incoming") {
    switch (t.status) {
      case "completed":
        return `Done (asked by ${who})`;
      case "failed":
        return `Failed (asked by ${who})`;
      case "cancelled":
        return `Stopped (asked by ${who})`;
      case "waiting_for_approval":
        return `Waiting for approval (asked by ${who})`;
      default:
        return `Working on it for ${who}`;
    }
  }
  switch (t.status) {
    case "pending":
      return "Not sent yet — waiting for the connection";
    case "sent":
      return `Sent to ${who}`;
    case "queued":
      return `${who} is offline — it will get this if it comes online within 10 minutes`;
    case "accepted":
      return `${who} accepted the task`;
    case "running":
      return `Running on ${who}`;
    case "waiting_for_approval":
      return `${who} is waiting for your OK`;
    case "paused":
      return `Paused on ${who}`;
    case "completed":
      return `Done — completed on ${who}`;
    case "ended":
      return `${who} stopped without a verified result — check it there`;
    case "failed":
      return `Failed on ${who}`;
    case "cancelled":
      return "Stopped";
    case "rejected":
      return `${who} didn't take it`;
    case "timed_out":
      return `${who} didn't respond in time, so it wasn't done`;
  }
}

export function taskTone(t: RemoteTask): "ok" | "warn" | "bad" {
  if (t.status === "completed") return "ok";
  if (["failed", "rejected", "timed_out", "ended"].includes(t.status)) return "bad";
  return "warn";
}

export function isActive(t: RemoteTask): boolean {
  return !["completed", "ended", "failed", "cancelled", "rejected", "timed_out"].includes(t.status);
}

export function keyProtectionText(p: string): string {
  if (p === "dpapi") return "Protected by Windows (DPAPI) for your account";
  if (p === "android-keystore") return "Protected by the Android Keystore";
  return "Stored without OS protection (no secure key store on this platform)";
}
