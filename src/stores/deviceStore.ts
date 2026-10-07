import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { api } from "@/services/api";
import { BackendError, hasBackend } from "@/services/backend";
import type { DeviceEvent, DevicesOverview, PairingCode, Platform, RemoteTask } from "@/types/devices";
import type { TaskControl } from "@/types/task";

export interface PairingRequest {
  pairingId: string;
  deviceName: string;
  platform: Platform;
}

export interface LocalApproval {
  callId: string;
  requestId: string;
  deviceName: string;
  title: string;
  description: string;
}

export interface Notice {
  id: number;
  title: string;
  body: string;
}

interface DeviceStore {
  overview: DevicesOverview | null;
  error: string | null;
  busy: boolean;
  /** The code this device shows while waiting for a new device. */
  pairing: PairingCode | null;
  /** A device typed the code and waits for the user's OK. */
  pairingRequests: PairingRequest[];
  pairingResult: { ok: boolean; message: string } | null;
  /** Actions in tasks other devices sent here, waiting for approval (this device's own prompt). */
  localApprovals: LocalApproval[];
  notices: Notice[];
  connect: () => () => void;
  refresh: () => Promise<void>;
  configure: (relayUrl: string | null, enabled: boolean, syncMemory: boolean) => Promise<boolean>;
  startPairing: () => Promise<void>;
  cancelPairing: () => Promise<void>;
  confirmPairing: (pairingId: string, allow: boolean) => Promise<void>;
  join: (code: string) => Promise<boolean>;
  revoke: (deviceId: string) => Promise<void>;
  forget: (deviceId: string) => Promise<void>;
  renameThis: (name: string) => Promise<void>;
  sendTask: (deviceId: string, objective: string) => Promise<RemoteTask | null>;
  control: (requestId: string, action: TaskControl) => Promise<void>;
  /** Answer another device's request (signed on this device). */
  answer: (callId: string, approve: boolean) => Promise<void>;
  /** Answer, here, an approval for a task another device sent here. */
  answerLocal: (callId: string, approve: boolean) => Promise<void>;
  dismissNotice: (id: number) => void;
  clearResult: () => void;
}

let noticeId = 0;
let refreshTimer: ReturnType<typeof setTimeout> | undefined;

export const useDeviceStore = create<DeviceStore>((set, get) => {
  const run = async <T>(f: () => Promise<T>): Promise<T | null> => {
    set({ busy: true, error: null });
    try {
      return await f();
    } catch (err) {
      set({ error: BackendError.from(err).message });
      return null;
    } finally {
      set({ busy: false });
      void get().refresh();
    }
  };

  const onEvent = (e: DeviceEvent) => {
    switch (e.type) {
      case "notice":
        set({ notices: [...get().notices, { id: ++noticeId, title: e.title, body: e.body }].slice(-4) });
        return;
      case "pairing_request":
        set({ pairingRequests: [...get().pairingRequests.filter((p) => p.pairingId !== e.pairingId), e] });
        return;
      case "pairing_done":
        set({ pairingResult: { ok: e.ok, message: e.message }, pairing: null });
        break;
      case "local_approval":
        set({ localApprovals: [...get().localApprovals.filter((a) => a.callId !== e.callId), e] });
        return;
      case "approval_closed":
        set({ localApprovals: get().localApprovals.filter((a) => a.callId !== e.callId) });
        break;
    }
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(() => void get().refresh(), 150);
  };

  return {
    overview: null,
    error: null,
    busy: false,
    pairing: null,
    pairingRequests: [],
    pairingResult: null,
    localApprovals: [],
    notices: [],

    connect: () => {
      if (!hasBackend()) return () => undefined;
      let unlisten: (() => void) | undefined;
      let closed = false;
      void listen<DeviceEvent>("device-event", (e) => onEvent(e.payload)).then((u) => {
        if (closed) u();
        else unlisten = u;
      });
      void get().refresh();
      return () => {
        closed = true;
        unlisten?.();
      };
    },

    refresh: async () => {
      try {
        set({ overview: await api.devicesOverview() });
      } catch (err) {
        // Still starting up: not an error the user has to act on yet.
        const e = BackendError.from(err);
        if (!/starting up/.test(e.message)) set({ error: e.message });
      }
    },

    configure: async (relayUrl, enabled, syncMemory) => (await run(() => api.devicesConfigure(relayUrl, enabled, syncMemory))) !== null,

    startPairing: async () => {
      set({ pairingResult: null });
      const code = await run(() => api.devicesStartPairing());
      if (code) set({ pairing: code });
    },

    cancelPairing: async () => {
      set({ pairing: null });
      await run(() => api.devicesCancelPairing());
    },

    confirmPairing: async (pairingId, allow) => {
      set({ pairingRequests: get().pairingRequests.filter((p) => p.pairingId !== pairingId), pairing: null });
      await run(() => api.devicesConfirmPairing(pairingId, allow));
    },

    join: async (code) => {
      set({ pairingResult: null });
      const d = await run(() => api.devicesJoin(code));
      return d !== null;
    },

    revoke: async (deviceId) => void (await run(() => api.devicesRevoke(deviceId))),
    forget: async (deviceId) => void (await run(() => api.devicesForget(deviceId))),
    renameThis: async (name) => void (await run(() => api.devicesRenameThis(name))),
    sendTask: (deviceId, objective) => run(() => api.devicesSendTask(deviceId, objective)),
    control: async (requestId, action) => void (await run(() => api.devicesControlTask(requestId, action))),
    answer: async (callId, approve) => void (await run(() => api.devicesAnswerApproval(callId, approve))),

    answerLocal: async (callId, approve) => {
      set({ localApprovals: get().localApprovals.filter((a) => a.callId !== callId) });
      const delivered = await run(() => api.respondToolApproval(callId, approve));
      if (delivered === false) set({ error: "That request already ended (answered on the other device, or it expired)." });
    },

    dismissNotice: (id) => set({ notices: get().notices.filter((n) => n.id !== id) }),
    clearResult: () => set({ pairingResult: null }),
  };
});
