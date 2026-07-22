import { ChevronRight } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode } from "react";

interface SidebarDisclosureProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "aria-expanded" | "children"> {
  expanded: boolean;
  children?: ReactNode;
  touchMode?: boolean;
}

export function SidebarDisclosure({ expanded, children, touchMode = false, className, ...buttonProps }: SidebarDisclosureProps) {
  return (
    <button
      type="button"
      {...buttonProps}
      aria-expanded={expanded}
      className={`sidebar-disclosure${className ? ` ${className}` : ""}`}
      data-touch-mode={touchMode ? "true" : undefined}
    >
      <ChevronRight className="sidebar-disclosure-glyph" aria-hidden />
      {children}
    </button>
  );
}
