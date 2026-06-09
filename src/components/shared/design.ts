export const UI_BUTTON = {
  base: "ct-btn",
  primary: "ct-btn ct-btn-primary",
  ghost: "ct-btn ct-btn-ghost",
  accent: "ct-btn ct-btn-accent",
  compact: "ct-btn ct-btn-compact",
  danger: "ct-btn ct-btn-danger",
  icon: "ct-btn ct-btn-icon ct-btn-ghost",
  sm: "ct-btn ct-btn-sm",
  lg: "ct-btn ct-btn-lg",
} as const;

export const UI_INPUT = {
  base: "ct-input",
  compact: "ct-input ct-input--compact",
  mono: "ct-input ct-input--compact ct-input--mono ct-text-mono",
  select: "ct-input ct-input--compact",
  full: "w-full ct-input",
  fullCompact: "w-full ct-input ct-input--compact",
  fullMono: "w-full ct-input ct-input--compact ct-input--mono ct-text-mono",
} as const;

export const UI_TEXT = {
  meta: "ct-meta",
  metaMuted: "ct-meta-muted",
  sectionLabel: "text-[10px] opacity-60 mb-[3px]",
  sectionTitle: "text-[10px] uppercase tracking-wider text-[var(--ct-muted)]",
  detailMuted: "text-[10px] text-[var(--ct-muted-fg)]",
} as const;

export const UI_PANEL = {
  toolbar: "ct-toolbar",
  toolbarTitle: "ct-toolbar-title",
  card: "ct-panel-card",
  cardSoft: "ct-panel-card ct-panel-card--soft",
  kbd: "ct-kbd",
  smallPill: "ct-small-pill",
  section: "text-[10px] flex flex-col gap-1",
  sectionLabel: "text-[var(--ct-muted)] uppercase tracking-wide text-[9px]",
  drawerBase: "fixed top-9 right-0 bottom-0 bg-[var(--ct-titlebar-bg)] border-l border-[var(--ct-border)] p-4 flex flex-col gap-3 z-50 overflow-auto",
  surfaceCard: "bg-[var(--ct-surface)] border-l border-[var(--ct-border)] text-[var(--ct-fg)]",
  drawerClose: "ct-btn ct-btn-ghost !w-7 !h-7 !leading-none text-[var(--ct-fg)]",
} as const;

export const tabClass = (active: boolean, touch: boolean) =>
  `ct-panel-tab ${active ? "ct-panel-tab--active" : "ct-panel-tab--idle"} ${touch ? "ct-panel-tab--touch px-3.5" : "px-3 py-2"} shrink-0 border-b-2 bg-transparent cursor-pointer capitalize`;
