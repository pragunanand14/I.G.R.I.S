interface SegmentedProps<T extends string> {
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
  label: string;
  disabled?: boolean;
}

export function Segmented<T extends string>({ value, options, onChange, label, disabled }: SegmentedProps<T>) {
  return (
    <div role="radiogroup" aria-label={label} className="inline-flex rounded-lg border border-line bg-surface p-0.5">
      {options.map((opt) => {
        const active = opt.value === value;
        return (
          <button
            key={opt.value}
            type="button"
            role="radio"
            aria-checked={active}
            disabled={disabled}
            onClick={() => onChange(opt.value)}
            className={`rounded-md px-3 py-1 text-xs font-medium transition-colors disabled:opacity-40 ${
              active ? "bg-surface-strong text-fg shadow-sm" : "text-muted hover:text-fg"
            }`}
          >
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}
