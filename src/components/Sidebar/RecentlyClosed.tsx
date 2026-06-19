import { memo, useState } from "react";
import { RotateCcw, ChevronRight } from "lucide-react";
import type { Workspace } from "../../lib/types";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";

interface RecentlyClosedProps {
  workspace: Workspace;
  /** Reopen the ring entry at `index` (0 = most recent). */
  onReopen: (index: number) => void;
  touchMode?: boolean;
  uiFontScale?: number;
}

/** Compact, collapsible "Recently closed" list rendered under a workspace's
 *  tab list. Newest first (matches the backend ring). Renders nothing when the
 *  ring is empty — no dead affordance. */
function RecentlyClosedInner({ workspace, onReopen, touchMode = false, uiFontScale }: RecentlyClosedProps) {
  const entries = workspace.recently_closed ?? [];
  const [open, setOpen] = useState(false);
  if (entries.length === 0) return null;

  const text = pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  const metaText = pickUiFontScaleValue(uiFontScale, ["text-[9px]", "text-[10px]", "text-[11px]", "text-xs"]);

  return (
    <div data-testid="recently-closed" className="mx-2 mt-0.5 mb-1">
      <button
        onClick={() => setOpen((v) => !v)}
        className={`flex items-center gap-1 w-full px-2 ${touchMode ? "py-2" : "py-1"} rounded-md text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] transition-colors`}
        title="Recently closed tabs"
      >
        <ChevronRight
          size={10}
          strokeWidth={1.5}
          className="shrink-0 transition-transform duration-200"
          style={{ transform: open ? "rotate(90deg)" : "rotate(0deg)" }}
        />
        <RotateCcw size={10} strokeWidth={1.5} className="shrink-0" />
        <span className={`${metaText} font-medium uppercase tracking-wide`}>
          Recently closed ({entries.length})
        </span>
      </button>
      {open && (
        <div className="mt-0.5">
          {entries.map((entry, i) => (
            <button
              key={`${entry.pane_id}-${i}`}
              data-testid="recently-closed-item"
              data-index={i}
              onClick={() => onReopen(i)}
              title={`Reopen "${entry.title || "Untitled"}"`}
              className={`flex items-center gap-2 w-full pl-6 pr-2 ${touchMode ? "py-2" : "py-1"} rounded-md text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors`}
            >
              <span className={`${text} truncate min-w-0 flex-1 text-left`}>
                {entry.title || "Untitled"}
              </span>
              {entry.model && (
                <span className={`${metaText} shrink-0 px-1 rounded bg-[var(--ct-active)] text-[var(--ct-muted)]`}>
                  {entry.model}
                </span>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export const RecentlyClosed = memo(RecentlyClosedInner);
