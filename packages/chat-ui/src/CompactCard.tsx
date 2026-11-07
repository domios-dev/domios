import { ChevronRight } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { useCompactCard } from "./useCompactToolRow";

/** A row's facts: the one key fact on a compact touch surface, the full set elsewhere and in the full-screen reader. */
export function CardFacts({ full, brief }: { full: ReactNode; brief: ReactNode }) {
  return <>{useCompactCard().brief ? brief : full}</>;
}

/** A card's trailing expand chevron. On a compact touch surface the row is the disclosure, so this duplicate control is dropped. */
export function CardChevron({ expanded, label, onToggle, controls, className = "flex shrink-0 items-center justify-center", style = { color: "var(--ct-muted-fg)" } }: {
  expanded: boolean;
  label: string;
  onToggle: () => void;
  controls?: string;
  className?: string;
  style?: CSSProperties;
}) {
  if (useCompactCard().compact) return null;
  return <button type="button" aria-label={label} aria-expanded={expanded} aria-controls={controls} onClick={onToggle} className={className} style={style}>
    <ChevronRight aria-hidden size={14} className={`shrink-0 ${expanded ? "rotate-90" : ""}`} />
  </button>;
}
