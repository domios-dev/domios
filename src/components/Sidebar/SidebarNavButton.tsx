import type { ReactNode, Ref } from "react";

/** One entry in the sidebar footer: an icon, a label when there is room, and the current-view highlight. */
export function SidebarNavButton({ testId, label, title, icon, current, onClick, compact, touchMode, labelClassName, grow = false, ref }: {
  testId: string;
  label: string;
  title?: string;
  icon: ReactNode;
  current: boolean;
  onClick?: () => void;
  compact: boolean;
  touchMode: boolean;
  labelClassName: string;
  /** Shares its row with a trailing control, so it fills the space that is left. */
  grow?: boolean;
  ref?: Ref<HTMLButtonElement>;
}) {
  const size = compact
    ? "justify-center w-8 h-8"
    : `gap-2 ${grow ? "flex-1 min-w-0" : "w-full"} px-2 ${touchMode ? "min-h-[44px] py-2.5" : "py-1.5"}`;
  const tone = current
    ? "bg-[var(--ct-active)] text-[var(--ct-fg)]"
    : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]";
  return (
    <button
      ref={ref}
      type="button"
      data-testid={testId}
      aria-current={current ? "page" : undefined}
      onClick={onClick}
      className={`flex items-center ${size} rounded-md transition-colors ${tone}`}
      title={title ?? label}
    >
      {icon}
      {!compact && <span className={labelClassName}>{label}</span>}
    </button>
  );
}

export function MemoryGraphIcon({ size }: { size: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="shrink-0" aria-hidden="true">
      <circle cx="12" cy="5" r="2" />
      <circle cx="5" cy="19" r="2" />
      <circle cx="19" cy="19" r="2" />
      <circle cx="12" cy="13" r="2" />
      <line x1="12" y1="7" x2="12" y2="11" />
      <line x1="10.5" y1="14.5" x2="6.5" y2="17.5" />
      <line x1="13.5" y1="14.5" x2="17.5" y2="17.5" />
    </svg>
  );
}

export function MeshIcon({ size }: { size: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="shrink-0" aria-hidden="true">
      <circle cx="12" cy="12" r="2" />
      <circle cx="4" cy="6" r="2" />
      <circle cx="20" cy="6" r="2" />
      <circle cx="4" cy="18" r="2" />
      <circle cx="20" cy="18" r="2" />
      <line x1="6" y1="6" x2="10.5" y2="11" />
      <line x1="18" y1="6" x2="13.5" y2="11" />
      <line x1="6" y1="18" x2="10.5" y2="13" />
      <line x1="18" y1="18" x2="13.5" y2="13" />
    </svg>
  );
}

export function NeighborsIcon({ size }: { size: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="shrink-0" aria-hidden="true">
      <circle cx="12" cy="12" r="8.5" strokeWidth="1.5" strokeDasharray="1.5 3.2" />
      <circle cx="12" cy="12" r="2.6" />
      <circle cx="12" cy="3.5" r="1.8" />
      <circle cx="19.4" cy="16.2" r="1.8" />
      <circle cx="4.6" cy="16.2" r="1.8" />
    </svg>
  );
}
