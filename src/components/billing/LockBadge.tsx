import { Lock } from "lucide-react";
import type { LockedView } from "./freeLimitModel";

/** Non-interactive mark: it sits inside the row's own activation, whose click opens the plans modal. */
export function LockBadge({ view, className = "" }: { view: LockedView; className?: string }) {
  if (!view.locked) return null;
  return (
    <span role="img" aria-label={view.label} title={`${view.label}. Upgrade to unlock.`} data-testid="lock-badge" className={`shrink-0 inline-flex items-center text-[var(--ct-muted-fg)] ${className}`}>
      <Lock size={12} strokeWidth={1.75} aria-hidden="true" />
    </span>
  );
}

/** Covers locked content and takes the click the content would have taken. */
export function LockedOverlay({ view, onActivate }: { view: LockedView; onActivate: () => void }) {
  if (!view.locked) return null;
  return (
    <button
      type="button"
      data-testid="lock-overlay"
      aria-label={`${view.label}. View plans to unlock`}
      onClick={onActivate}
      className="absolute inset-0 z-20 flex flex-col items-center justify-center gap-2 border-0 bg-[var(--ct-bg)]/70 text-[var(--ct-fg)] backdrop-blur-[1px] cursor-pointer"
    >
      <Lock size={20} strokeWidth={1.5} aria-hidden="true" />
      <span className="text-sm">{view.label}</span>
      <span className="text-xs text-[var(--ct-muted-fg)]">View plans to unlock</span>
    </button>
  );
}
