import type { TabActivity } from "../../lib/tabActivity";
import type { Tab } from "../../lib/types";
import { managedTabPresentation } from "../../lib/managedTab";

export function ManagedTabBar({ managed, activity, onStop }: { managed: Tab["managed"]; activity: TabActivity; onStop: () => void }) {
  const view = managedTabPresentation(managed);
  if (!view.label) return null;
  return (
    <div className="flex items-center justify-between gap-3 border-t border-[var(--ct-border)] bg-[var(--ct-bg)] px-4 py-3">
      <span role="status" aria-label={view.label}>{view.label}</span>
      {view.locked && activity.canStop && <button type="button" onClick={onStop} aria-label="Stop generation">Stop</button>}
    </div>
  );
}
