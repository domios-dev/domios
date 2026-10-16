import { roleBadgeTestId } from "./orchestrationSections";

interface RoleBadgeProps {
  label: string;
  /** Model, moved off the badge face into the tooltip so it stays discoverable. */
  title?: string;
  className?: string;
}

export function RoleBadge({ label, title, className = "" }: RoleBadgeProps) {
  return (
    <span
      data-testid={roleBadgeTestId(label)}
      data-role-badge={label}
      className={`sidebar-role-badge text-[10px] leading-3 font-normal text-[var(--ct-accent)] ${className}`}
      title={title}
    >
      {label.charAt(0).toUpperCase() + label.slice(1).toLowerCase()}
    </span>
  );
}
