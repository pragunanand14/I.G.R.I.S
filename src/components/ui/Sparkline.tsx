interface SparklineProps {
  values: number[];
  max?: number;
  height?: number;
  label: string;
}

/** Lightweight SVG sparkline; values are percentages by default. */
export function Sparkline({ values, max = 100, height = 48, label }: SparklineProps) {
  const width = 240;
  if (values.length < 2) {
    return (
      <div className="flex items-center text-xs text-faint" style={{ height }}>
        Collecting samples…
      </div>
    );
  }
  const step = width / (values.length - 1);
  const pts = values.map((v, i) => `${(i * step).toFixed(1)},${(height - (Math.min(v, max) / max) * (height - 2) - 1).toFixed(1)}`);
  const line = pts.join(" ");
  const area = `0,${height} ${line} ${width},${height}`;
  return (
    <svg role="img" aria-label={label} viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" className="w-full" style={{ height }}>
      <polygon points={area} fill="rgb(var(--accent-rgb) / 0.12)" />
      <polyline points={line} fill="none" stroke="var(--accent)" strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}
