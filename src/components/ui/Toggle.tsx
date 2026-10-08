interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
}

export function Toggle({ checked, onChange, label, disabled }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative h-6 w-10 shrink-0 rounded-full transition-colors duration-300 disabled:opacity-40 ${checked ? "bg-accent" : "bg-surface-strong"}`}
    >
      <span
        className={`absolute top-1/2 left-[3px] size-[18px] -translate-y-1/2 rounded-full bg-white shadow-sm transition-transform duration-300 ease-[var(--ease-out)] ${
          checked ? "translate-x-4" : "translate-x-0"
        }`}
      />
    </button>
  );
}
