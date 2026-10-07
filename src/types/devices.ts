/** Cross-device IGRIS — mirrors `igris_core::device` (camelCase). */

export type Platform = "windows" | "android" | "linux" | "macos" | "other";

export type Capability =
  | "tasks"
  | "memory"
  | "productivity"
  | "files"
  | "desktop_apps"
  | "terminal"
  | "browser"
  | "computer_control"
  | "screenshots"
  | "phone_apps"
  | "phone_status"
  | "web";

export interface ThisDevice {
  deviceId: string;
  name: string;
  platform: Platform;
  capabilities: Capability[];
  /** "dpapi", "android-keystore" or "none". */
  keyProtection: string;
}

export interface PairedDevice {
  deviceId: string;
  ownerId: string;
  name: string;
  platform: Platform;
  capabilities: Capability[];
  trusted: boolean;
  revokedAt: string | null;
  lastSeen: string | null;
  createdAt: string;
  updatedAt: string;
  online: boolean;
}

export interface DeviceSettings {
  relayUrl: string | null;
  enabled: boolean;
  syncMemory: boolean;
}

export interface LinkStatus {
  state: "off" | "connecting" | "connected" | "offline";
  detail: string | null;
  relayUrl: string | null;
}

export type RemoteStatus =
  | "pending"
  | "sent"
  | "queued"
  | "accepted"
  | "running"
  | "waiting_for_approval"
  | "paused"
  | "completed"
  | "ended"
  | "failed"
  | "cancelled"
  | "rejected"
  | "timed_out";

export interface RemoteTask {
  requestId: string;
  direction: "outgoing" | "incoming";
  peerDeviceId: string;
  peerName: string;
  objective: string;
  conversationId: string | null;
  taskId: string | null;
  status: RemoteStatus;
  detail: string | null;
  result: string | null;
  lastSeq: number;
  createdAt: string;
  updatedAt: string;
}

export interface ApprovalRequest {
  requestId: string;
  taskId: string;
  callId: string;
  tool: string;
  title: string;
  description: string;
  permission: string;
  inputDigest: string;
  targetDevice: string;
  expiresAt: number;
}

export interface ApprovalView {
  request: ApprovalRequest;
  deviceName: string;
}

export interface DevicesOverview {
  thisDevice: ThisDevice;
  settings: DeviceSettings;
  status: LinkStatus;
  devices: PairedDevice[];
  tasks: RemoteTask[];
  approvals: ApprovalView[];
}

export interface PairingCode {
  code: string;
  expiresAt: number;
}

export interface DeviceAuditRow {
  id: number;
  at: string;
  peer: string | null;
  kind: string;
  detail: string | null;
}

export type DeviceEvent =
  | { type: "ready" }
  | ({ type: "status" } & LinkStatus)
  | { type: "presence"; deviceId: string; online: boolean }
  | { type: "devices_changed" }
  | ({ type: "task" } & RemoteTask)
  | ({ type: "approval_needed" } & ApprovalView)
  | { type: "approval_closed"; callId: string }
  | { type: "local_approval"; callId: string; requestId: string; deviceName: string; title: string; description: string }
  | { type: "pairing_request"; pairingId: string; deviceName: string; platform: Platform }
  | { type: "pairing_done"; ok: boolean; message: string }
  | { type: "notice"; title: string; body: string };
