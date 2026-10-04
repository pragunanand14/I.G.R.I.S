import { memo } from "react";
import type { CoreState } from "@/types/assistant";
import "./AiCore.css";

const TICKS = 72;
const BARS = 64;
const C = 200; // centre of the 400×400 viewBox

function polar(r: number, i: number, n: number) {
  const a = (i / n) * Math.PI * 2 - Math.PI / 2;
  return { x: C + r * Math.cos(a), y: C + r * Math.sin(a) };
}

// Geometry is static; precompute once.
const tickLines = Array.from({ length: TICKS }, (_, i) => {
  const major = i % 6 === 0;
  const a = polar(major ? 172 : 176, i, TICKS);
  const b = polar(184, i, TICKS);
  return { ...a, x2: b.x, y2: b.y, major };
});
const barAngles = Array.from({ length: BARS }, (_, i) => ({ i, deg: (i / BARS) * 360 }));

interface AiCoreProps {
  state: CoreState;
  size?: number;
}

/**
 * The IGRIS core. Purely presentational: it visualises the state it is given.
 * All animation is CSS transform/opacity so it stays GPU-cheap.
 */
export const AiCore = memo(function AiCore({ state, size = 320 }: AiCoreProps) {
  return (
    <div className={`ai-core ai-core--${state}`} style={{ width: size, height: size }} data-state={state} aria-hidden="true">
      <div className="ai-core__halo" />
      <svg viewBox="0 0 400 400" className="ai-core__svg">
        <defs>
          <radialGradient id="ai-core-glow" cx="50%" cy="50%" r="50%">
            <stop offset="0" stopColor="rgb(var(--core-rgb))" stopOpacity="0.95" />
            <stop offset="0.45" stopColor="rgb(var(--core-rgb))" stopOpacity="0.35" />
            <stop offset="1" stopColor="rgb(var(--core-rgb))" stopOpacity="0" />
          </radialGradient>
        </defs>

        <g className="ai-core__ticks">
          {tickLines.map((t, i) => (
            <line key={i} x1={t.x} y1={t.y} x2={t.x2} y2={t.y2} className={t.major ? "major" : undefined} />
          ))}
        </g>

        <circle cx={C} cy={C} r={156} className="ai-core__track" />
        <g className="ai-core__arc">
          <circle cx={C} cy={C} r={156} pathLength={100} strokeDasharray="18 32" />
        </g>
        <g className="ai-core__dash">
          <circle cx={C} cy={C} r={134} />
        </g>

        <g className="ai-core__pulses">
          <circle cx={C} cy={C} r={92} />
          <circle cx={C} cy={C} r={92} />
          <circle cx={C} cy={C} r={92} />
        </g>

        <g className="ai-core__bars">
          {barAngles.map((b) => (
            <g key={b.i} transform={`rotate(${b.deg} ${C} ${C})`}>
              <line x1={C} y1={C - 116} x2={C} y2={C - 100} style={{ animationDelay: `${((b.i * 7) % 11) * -0.09}s` }} />
            </g>
          ))}
        </g>

        <circle cx={C} cy={C} r={92} fill="url(#ai-core-glow)" className="ai-core__glow" />
        <circle cx={C} cy={C} r={30} className="ai-core__nucleus" />
      </svg>
    </div>
  );
});
