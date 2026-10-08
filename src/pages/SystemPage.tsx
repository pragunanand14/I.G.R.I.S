import type { ReactNode } from "react";
import { Meter } from "@/components/ui/Meter";
import { PageHeader } from "@/components/ui/PageHeader";
import { Panel } from "@/components/ui/Panel";
import { Sparkline } from "@/components/ui/Sparkline";
import { StatusDot } from "@/components/ui/StatusDot";
import { useAppStore } from "@/stores/appStore";
import { useSystemStore } from "@/stores/systemStore";
import { formatBytes, formatDuration, formatPercent, formatRate, percentOf } from "@/utils/format";

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-4 py-1.5 text-sm">
      <span className="text-muted">{label}</span>
      <span className="tabular truncate text-right text-fg" data-selectable>
        {children}
      </span>
    </div>
  );
}

function BigValue({ value, unit }: { value: string; unit?: string }) {
  return (
    <div className="tabular mb-3 text-3xl font-extralight text-fg">
      {value}
      {unit && <span className="ml-1 text-base text-muted">{unit}</span>}
    </div>
  );
}

export function SystemPage() {
  const backend = useAppStore((s) => s.backend);
  const { snapshot: s, error, cpuHistory, memoryHistory } = useSystemStore();

  if (backend !== "ready") {
    return (
      <div className="mx-auto max-w-3xl px-8 pt-8">
        <PageHeader title="System" description="Live hardware and OS telemetry." />
        <p className="text-sm text-danger">System telemetry requires the IGRIS desktop backend.</p>
      </div>
    );
  }

  if (!s) {
    return (
      <div className="mx-auto max-w-3xl px-8 pt-8">
        <PageHeader title="System" description="Live hardware and OS telemetry." />
        <p className="text-sm text-muted">{error ? `Telemetry unavailable: ${error}` : "Reading system telemetry…"}</p>
      </div>
    );
  }

  const memPct = percentOf(s.memory.usedBytes, s.memory.totalBytes);
  const swapPct = percentOf(s.memory.swapUsedBytes, s.memory.swapTotalBytes);
  const conn = s.network.connectivity;

  return (
    <div className="mx-auto max-w-6xl p-8">
      <PageHeader
        title="System"
        description="Live hardware and OS telemetry, read directly from the operating system."
        action={error ? <span className="text-xs text-danger">Last refresh failed: {error}</span> : undefined}
      />

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        <Panel title="Processor">
          <BigValue value={s.cpu.usagePercent == null ? "—" : s.cpu.usagePercent.toFixed(0)} unit="%" />
          <Sparkline values={cpuHistory} label="CPU usage history" />
          <div className="mt-3 border-t border-line pt-2">
            <Row label="Model">{s.cpu.brand || "—"}</Row>
            <Row label="Cores / threads">
              {s.cpu.physicalCores ?? "—"} / {s.cpu.logicalCores}
            </Row>
            <Row label="Frequency">{s.cpu.frequencyMhz ? `${(s.cpu.frequencyMhz / 1000).toFixed(2)} GHz` : "—"}</Row>
          </div>
        </Panel>

        <Panel title="Memory">
          <BigValue value={memPct == null ? "—" : memPct.toFixed(0)} unit="%" />
          <Sparkline values={memoryHistory} label="Memory usage history" />
          <div className="mt-3 border-t border-line pt-2">
            <Row label="Used">
              {formatBytes(s.memory.usedBytes)} / {formatBytes(s.memory.totalBytes)}
            </Row>
            <Row label="Available">{formatBytes(s.memory.availableBytes)}</Row>
            <Row label="Swap">
              {s.memory.swapTotalBytes > 0
                ? `${formatBytes(s.memory.swapUsedBytes)} / ${formatBytes(s.memory.swapTotalBytes)} (${formatPercent(swapPct)})`
                : "None"}
            </Row>
          </div>
        </Panel>

        <Panel title="Network">
          <div className="mb-3 flex items-center gap-2">
            <StatusDot tone={conn === "online" ? "ok" : conn === "offline" ? "error" : "idle"} />
            <span className="text-lg font-light text-fg">
              {conn === "online" ? "Internet reachable" : conn === "offline" ? "No internet" : "Checking…"}
            </span>
          </div>
          <Row label="Download">{formatRate(s.network.rxBytesPerSec)}</Row>
          <Row label="Upload">{formatRate(s.network.txBytesPerSec)}</Row>
          <Row label="Interfaces">{s.network.interfaceCount ?? "—"}</Row>
          <p className="mt-2 text-[11px] text-faint">Reachability is checked by connecting to public DNS resolvers every 15 s.</p>
        </Panel>

        <Panel title="Power">
          {s.battery ? (
            <>
              <BigValue value={s.battery.percent.toFixed(0)} unit="%" />
              <Meter value={s.battery.percent} label="Battery level" tone="accent" className="mb-3" />
              <Row label="State">{s.battery.state}</Row>
              {s.battery.timeToEmptySecs != null && <Row label="Time remaining">{formatDuration(s.battery.timeToEmptySecs)}</Row>}
              {s.battery.timeToFullSecs != null && <Row label="Time to full">{formatDuration(s.battery.timeToFullSecs)}</Row>}
            </>
          ) : (
            <p className="text-sm text-muted">No battery detected on this system.</p>
          )}
        </Panel>

        <Panel title="Host">
          <Row label="Hostname">{s.host.hostname ?? "—"}</Row>
          <Row label="OS">{[s.host.osName, s.host.osVersion].filter(Boolean).join(" ") || "—"}</Row>
          <Row label="Kernel">{s.host.kernelVersion ?? "—"}</Row>
          <Row label="Architecture">{s.host.arch}</Row>
          <Row label="Uptime">{formatDuration(s.uptimeSecs)}</Row>
        </Panel>

        <Panel title="Sensors">
          {s.temperatures.length > 0 ? (
            s.temperatures.slice(0, 8).map((t, i) => (
              <Row key={`${t.label}-${i}`} label={t.label}>
                <span className={t.criticalCelsius && t.celsius >= t.criticalCelsius * 0.9 ? "text-danger" : undefined}>
                  {t.celsius.toFixed(0)} °C
                </span>
              </Row>
            ))
          ) : (
            <p className="text-sm text-muted">This system does not expose temperature sensors to IGRIS.</p>
          )}
          <p className="mt-2 text-[11px] text-faint">GPU telemetry is not yet supported.</p>
        </Panel>

        <Panel title="Storage" className="lg:col-span-3">
          {s.disks.length === 0 ? (
            <p className="text-sm text-muted">No disks reported.</p>
          ) : (
            <div className="grid grid-cols-1 gap-x-8 gap-y-4 md:grid-cols-2">
              {s.disks.map((d) => {
                const used = d.totalBytes - d.availableBytes;
                const pct = percentOf(used, d.totalBytes);
                return (
                  <div key={`${d.name}-${d.mountPoint}`}>
                    <div className="mb-1.5 flex items-baseline justify-between gap-3 text-sm">
                      <span className="truncate text-fg" data-selectable>
                        {d.mountPoint}
                        <span className="ml-2 text-xs text-faint">
                          {d.fileSystem} · {d.kind}
                          {d.removable ? " · removable" : ""}
                        </span>
                      </span>
                      <span className="tabular shrink-0 text-xs text-muted">
                        {formatBytes(used)} / {formatBytes(d.totalBytes)}
                      </span>
                    </div>
                    <Meter value={pct} label={`${d.mountPoint} usage`} />
                  </div>
                );
              })}
            </div>
          )}
        </Panel>
      </div>
    </div>
  );
}
