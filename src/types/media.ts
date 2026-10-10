export type MediaAction = "play_pause" | "play" | "pause" | "next" | "previous" | "stop" | "volume_up" | "volume_down" | "mute";

/** What's playing on this device, as far as it reports. */
export interface NowPlaying {
  playing: boolean;
  /** There's a player to control (playing or paused). */
  active: boolean;
  title: string | null;
  artist: string | null;
  app: string | null;
}
