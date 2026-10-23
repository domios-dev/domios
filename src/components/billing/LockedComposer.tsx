import { Lock } from "lucide-react";
import { openLockedPlans } from "./freeLimitGate";
import type { LockedView } from "./freeLimitModel";

/** Stands in for the composer of a locked tab: nothing can be typed or sent, and the way forward is the plans modal. */
export function LockedComposer({ view }: { view: LockedView }) {
  if (!view.locked) return null;
  return (
    <div role="group" aria-label="Locked message composer" data-testid="locked-composer" className="ct-chat-bottom-bar flex items-center gap-2">
      <textarea
        disabled
        rows={1}
        aria-label="Message (locked)"
        placeholder={view.label}
        className="min-w-0 flex-1 resize-none rounded border border-[var(--ct-border)] bg-transparent px-2 py-1.5 text-sm opacity-60"
      />
      <button
        type="button"
        onClick={() => openLockedPlans(view)}
        className="inline-flex shrink-0 items-center gap-1.5 rounded border border-[var(--ct-border)] px-2.5 py-1.5 text-sm text-[var(--ct-fg)] hover:bg-[var(--ct-hover)]"
      >
        <Lock size={13} aria-hidden="true" />
        View plans to unlock
      </button>
    </div>
  );
}
