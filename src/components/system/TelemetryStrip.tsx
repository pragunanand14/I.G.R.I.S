import { Battery, BatteryCharging, Cpu, MemoryStick, Wifi, WifiOff } from "lucide-react";
import type { ReactNode } from "react";
import { Meter } from "@/components/ui/Meter";
import { useSystemStore } from "@/stores/systemStore";
import { formatBytes, formatPercent, percentOf } from "@/utils/format";

interface TileProps {
  icon: ReactNode;
  label: string;
  value: string;
  detail: string;
  meter?: number | null;
}

function Tile({ icon, label, value, detail, meter }: TileProps) {
  return (
    <div className="flex min-w-0 flex-col gap-2 px-5 py-3.5">
      <div className="flex items-center gap-2 text-faint">
        {icon}
        <span className="text-label">{label}</span>
      </div>
      <div className="tabular truncate text-xl font-light text-fg">{value}</div>
      {meter !== undefined ? <Meter value={meter} label={`${label} usage`} /> : <div className="h-1" />}
      <div className="tabular truncate text-[11px] text-muted">{detail}</div>
    </div>
  );
}

/** Home-screen telemetry summary. Shows "—" until real samples arrive. */
export function TelemetryStrip() {
  const snapshot = useSystemStore((s) => s.snapshot);
  const error = useSystemStore((s) => s.error);

  const cpu = snapshot?.cpu.usagePercent ?? null;
  const mem = snapshot ? percentOf(snapshot.memory.usedBytes, snapshot.memory.totalBytes) : null;
  const conn = snapshot?.network.connectivity ?? "unknown";
  const battery = snapshot?.battery ?? null;

  const iconCls = "size-3.5";
  return (
    <div className="w-full max-w-4xl">
      <div className="grid grid-cols-4 divide-x divide-line rounded-xl border border-line bg-surface backdrop-blur-sm">
        <Tile
          icon={<Cpu className={iconCls} />}
          label="CPU"
          value={formatPercent(cpu)}
          meter={cpu}
          detail={snapshot ? `${snapshot.cpu.logicalCores} threads` : "—"}
        />
        <Tile
          icon={<MemoryStick className={iconCls} />}
          label="RAM"
          value={formatPercent(mem)}
          meter={mem}
          detail={snapshot ? `${formatBytes(snapshot.memory.usedBytes)} / ${formatBytes(snapshot.memory.totalBytes)}` : "—"}
        />
        <Tile
          icon={conn === "offline" ? <WifiOff className={iconCls} /> : <Wifi className={iconCls} />}
          label="Network"
          value={!snapshot ? "—" : conn === "online" ? "Connected" : conn === "offline" ? "Offline" : "Checking…"}
          detail={snapshot ? `${snapshot.network.interfaceCount} interface${snapshot.network.interfaceCount === 1 ? "" : "s"}` : "—"}
        />
        <Tile
          icon={battery?.state === "charging" ? <BatteryCharging className={iconCls} /> : <Battery className={iconCls} />}
          label="Battery"
          value={!snapshot ? "—" : battery ? formatPercent(battery.percent) : "None"}
          meter={battery ? battery.percent : undefined}
          detail={!snapshot ? "—" : battery ? battery.state : "No battery detected"}
        />
      </div>
      {error && <p className="mt-2 text-center text-xs text-danger">Telemetry unavailable: {error}</p>}
    </div>
  );
}
