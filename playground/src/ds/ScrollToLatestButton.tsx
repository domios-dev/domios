import type { ButtonHTMLAttributes } from "react";
import { ChevronDown } from "lucide-react";
import "./scroll-to-latest.css";

export interface ScrollToLatestButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  compact?: boolean;
}

export function ScrollToLatestButton({ compact = false, title = "Scroll to latest", className = "", ...props }: ScrollToLatestButtonProps) {
  return <button type="button" data-compact={compact || undefined} title={title} aria-label={title} className={`ds-scroll-latest ${className}`} {...props}>
    <ChevronDown size={compact ? 13 : 16} strokeWidth={2.2} />
  </button>;
}
