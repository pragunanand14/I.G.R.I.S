// Mirrors `src-tauri/src/system/*.rs`. Keep in sync.
export type Connectivity = "unknown" | "online" | "offline";

export interface HostInfo {
  hostname: string | null;
  osName: string | null;
  osVersion: string | null;
  kernelVersion: string | null;
  arch: string;
}

export interface CpuInfo {
  usagePercent: number | null;
  brand: string;
  logicalCores: number;
  physicalCores: number | null;
  frequencyMhz: number | null;
}

export interface MemoryInfo {
  totalBytes: number;
  usedBytes: number;
  availableBytes: number;
  swapTotalBytes: number;
  swapUsedBytes: number;
}

export interface DiskInfo {
  name: string;
  mountPoint: string;
  fileSystem: string;
  kind: string;
  removable: boolean;
  totalBytes: number;
  availableBytes: number;
}

export interface NetworkInfo {
  connectivity: Connectivity;
  rxBytesPerSec: number | null;
  txBytesPerSec: number | null;
  /** `null` when the platform hides network interfaces (Android). */
  interfaceCount: number | null;
}

export type BatteryState = "charging" | "discharging" | "full" | "empty" | "paused" | "unknown";

export interface BatteryInfo {
  percent: number;
  state: BatteryState;
  timeToEmptySecs: number | null;
  timeToFullSecs: number | null;
}

export interface TemperatureInfo {
  label: string;
  celsius: number;
  criticalCelsius: number | null;
}

export interface SystemSnapshot {
  timestampMs: number;
  host: HostInfo;
  uptimeSecs: number;
  cpu: CpuInfo;
  memory: MemoryInfo;
  disks: DiskInfo[];
  network: NetworkInfo;
  battery: BatteryInfo | null;
  temperatures: TemperatureInfo[];
}
