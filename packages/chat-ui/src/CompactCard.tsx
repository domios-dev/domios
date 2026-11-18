import { ChevronRight } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { useCompactCard } from "./useCompactToolRow";

/** A row's facts: the one key fact on a compact touch surface, the full set elsewhere and in the full-screen reader. */
export function CardFacts({ full, brief }: { full: ReactNode; brief: ReactNode }) {
  return <>{useCompactCard().brief ? brief : full}</>;
}

/** Compact rows omit the duplicate icon-only toggle and retain labeled disclosures. */
export function CardChevron({ expanded, label, onToggle, controls, children, className = "flex shrink-0 items-center justify-center", style = { color: "var(--ct-muted-fg)" } }: {
  expanded: boolean;
  label: string;
  onToggle: () => void;
  controls?: string;
  children?: ReactNode;
  className?: string;
  style?: CSSProperties;
}) {
  if (useCompactCard().compact && !children) return null;
  return <button type="button" aria-label={label} aria-expanded={expanded} aria-controls={controls} onClick={onToggle} className={className} style={style}>
    <ChevronRight aria-hidden size={14} className={`shrink-0 ${expanded ? "rotate-90" : ""}`} />
    {children}
  </button>;
}
