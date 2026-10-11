/** Shared classes for the People and Spending panels (computer or phone look). */
export function lifeStyles(phone: boolean) {
  return {
    card: phone
      ? "m-card space-y-3 px-4 py-4"
      : "space-y-3 rounded-2xl bg-surface p-4",
    title: phone
      ? "text-[16px] font-semibold"
      : "text-sm font-semibold text-fg",
    text: phone ? "text-[15px]" : "text-sm text-fg",
    muted: phone ? "m-muted text-sm" : "text-xs text-muted",
    input: phone
      ? "m-input"
      : "w-full rounded-xl bg-surface-strong px-3 py-2 text-sm text-fg placeholder:text-faint focus:outline-none",
    chip: phone
      ? "m-chip"
      : "flex items-center gap-1 rounded-full bg-surface-strong px-3 py-1.5 text-xs font-medium text-fg hover:bg-surface-hover disabled:opacity-40",
    primary: phone
      ? "m-button m-button-primary w-full"
      : "rounded-full bg-accent px-4 py-2 text-sm font-semibold text-bg hover:opacity-90 disabled:opacity-40",
  };
}
