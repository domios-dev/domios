import type { ReactNode } from "react";
import { FloatingTooltip } from "../shared/FloatingTooltip";

const reason = "Rename it on that machine, or when it's back online.";

export function UnavailableMachineRename({ label, children, className = "" }: {
  label: string;
  children: ReactNode;
  className?: string;
}) {
  return <FloatingTooltip content={reason} focusable={false} className="inline-flex shrink-0"
    tooltipStyle={{ width: "max-content", maxWidth: "min(20rem, calc(100vw - 16px))" }}>
    <button type="button" aria-label={label} aria-disabled="true" aria-description={reason}
      className={`cursor-not-allowed text-[var(--ct-muted)] ${className}`}>{children}</button>
  </FloatingTooltip>;
}
