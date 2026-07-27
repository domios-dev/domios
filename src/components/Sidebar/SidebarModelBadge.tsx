interface SidebarModelBadgeProps {
  label: string;
  title?: string;
  className?: string;
}

export function SidebarModelBadge({ label, title, className = "" }: SidebarModelBadgeProps) {
  return (
    <span
      className={`sidebar-pane-badge px-1 py-0 text-[8px] leading-tight rounded font-mono uppercase tracking-wider border border-[var(--ct-border)] text-[var(--ct-muted-fg)] ${className}`}
      title={title}
    >
      {label}
    </span>
  );
}
