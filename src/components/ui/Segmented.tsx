import { useLayoutEffect, useRef, useState } from "react";

interface SegmentedProps<T extends string> {
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
  label: string;
  disabled?: boolean;
}

/** A row of options; the highlight slides to the chosen one. */
export function Segmented<T extends string>({ value, options, onChange, label, disabled }: SegmentedProps<T>) {
  const refs = useRef<Record<string, HTMLButtonElement | null>>({});
  const [thumb, setThumb] = useState<{ x: number; w: number } | null>(null);
  useLayoutEffect(() => {
    const el = refs.current[value];
    if (el) setThumb({ x: el.offsetLeft, w: el.offsetWidth });
  }, [value, options]);

  return (
    <div role="radiogroup" aria-label={label} className="relative inline-flex rounded-xl bg-surface-strong p-1">
      {thumb && (
        <span
          aria-hidden="true"
          className="absolute top-1 bottom-1 left-0 rounded-lg bg-elevated shadow-sm transition-[transform,width] duration-300 ease-[var(--ease-out)]"
          style={{ transform: `translateX(${thumb.x}px)`, width: thumb.w }}
        />
      )}
      {options.map((opt) => {
        const active = opt.value === value;
        return (
          <button
            key={opt.value}
            ref={(el) => {
              refs.current[opt.value] = el;
            }}
            type="button"
            role="radio"
            aria-checked={active}
            disabled={disabled}
            onClick={() => onChange(opt.value)}
            className={`relative rounded-lg px-3.5 py-1.5 text-xs font-medium transition-colors duration-200 disabled:opacity-40 ${
              active ? "text-fg" : "text-muted hover:text-fg"
            }`}
          >
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}
