/** A smart-home device set up in IGRIS (no secrets ever reach the UI). */
export interface HomeDevice {
  id: string;
  kind: "lg_tv";
  name: string;
  host: string;
  /** Its network address is known, so it can be switched on remotely. */
  canPowerOn: boolean;
  createdAt: string;
}

export interface FoundDevice {
  kind: "lg_tv";
  host: string;
  name: string;
}

export interface TvApp {
  id: string;
  title: string;
}

/** What the TV reported; `on: false` when it's off or can't be reached. */
export interface TvState {
  on: boolean;
  volume: number | null;
  muted: boolean | null;
  app: string | null;
  apps: TvApp[];
  inputs: TvApp[];
}

export type TvActionName =
  | "power_on"
  | "power_off"
  | "volume_up"
  | "volume_down"
  | "set_volume"
  | "mute"
  | "unmute"
  | "channel_up"
  | "channel_down"
  | "play"
  | "pause"
  | "stop"
  | "rewind"
  | "fast_forward"
  | "open_app"
  | "switch_input"
  | "button"
  | "show_message";

/** One touchpad input for the TV's cursor (relative pixels). */
export type PointerEvent = { type: "move"; dx: number; dy: number } | { type: "click" } | { type: "scroll"; dx: number; dy: number };
