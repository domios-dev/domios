import { ChevronLeft } from "lucide-react";

/** The one way back out of a view: a quiet pill that names where it returns in its accessible label. */
export function BackPill({ label, onBack, ariaLabel }: { label?: string; onBack: () => void; ariaLabel?: string }) {
  const name = ariaLabel ?? (label ? `Back to ${label}` : "Back");
  return (
    <button type="button" className="ct-bar-pill" aria-label={name} title={name} onClick={onBack}>
      <ChevronLeft size={14} aria-hidden="true" />Back
    </button>
  );
}
