export const MODEL_BADGE_STYLE = "px-1 py-0 text-[8px] leading-tight rounded font-mono border border-[var(--ct-border)]";

interface SidebarModelBadgeProps {
  label: string;
  title?: string;
  className?: string;
}

export function SidebarModelBadge({ label, title, className = "" }: SidebarModelBadgeProps) {
  return (
    <span
      className={`sidebar-tab-badge ${MODEL_BADGE_STYLE} uppercase tracking-wider text-[var(--ct-muted-fg)] ${className}`}
      title={title}
      aria-label={title}
    >
      {label}
    </span>
  );
}
