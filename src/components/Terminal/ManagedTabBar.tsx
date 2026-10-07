import type { Tab } from "../../lib/types";
import { managedTabPresentation } from "../../lib/managedTab";

export function ManagedTabBar({ managed, onStop }: { managed: Tab["managed"]; onStop: () => void }) {
  const view = managedTabPresentation(managed);
  if (!view.label) return null;
  return (
    <div className="flex items-center justify-between gap-3 border-t border-[var(--ct-border)] bg-[var(--ct-bg)] px-4 py-3">
      <span role="status" aria-label={view.label}>{view.label}</span>
      {view.locked && <button type="button" onClick={onStop} aria-label="Stop generation">Stop</button>}
    </div>
  );
}
