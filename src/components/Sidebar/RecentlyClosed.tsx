import { memo, useState, useCallback } from "react";
import { createPortal } from "react-dom";
import { RotateCcw, ChevronRight } from "lucide-react";
import type { ClosedPane } from "../../lib/types";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";

interface AggEntry {
  entry: ClosedPane;
  workspaceId: string;
  index: number;
  orphaned: boolean;
}

interface RecentlyClosedProps {
  entries: AggEntry[];
  showWorkspace: boolean;
  onReopen: (workspaceId: string, index: number, orphaned: boolean, paneId: string) => void;
  touchMode?: boolean;
  uiFontScale?: number;
}

interface TooltipState {
  text: string;
  x: number;
  y: number;
}

function RecentlyClosedInner({
  entries,
  showWorkspace,
  onReopen,
  touchMode = false,
  uiFontScale,
}: RecentlyClosedProps) {
  const [open, setOpen] = useState(false);
  const [tooltip, setTooltip] = useState<TooltipState | null>(null);

  const handleMouseMove = useCallback((text: string, e: React.MouseEvent) => {
    setTooltip({ text, x: e.clientX, y: e.clientY });
  }, []);

  const handleMouseLeave = useCallback(() => {
    setTooltip(null);
  }, []);

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
          Recently closed
        </span>
      </button>
      {open && (
        <div
          className="mt-0.5 overflow-y-auto"
          style={{ maxHeight: 125 }}
        >
          {entries.map(({ entry, workspaceId, index, orphaned }) => (
            <button
              key={`${entry.pane_id}-${workspaceId}-${index}`}
              data-testid="recently-closed-item"
              onClick={() => { setTooltip(null); onReopen(workspaceId, index, orphaned, entry.pane_id); }}
              onMouseMove={entry.description ? (e) => handleMouseMove(entry.description!, e) : undefined}
              onMouseLeave={entry.description ? handleMouseLeave : undefined}
              className={`flex items-center gap-2 w-full pl-6 pr-2 ${touchMode ? "py-2" : "py-1"} rounded-md text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors`}
            >
              <span className={`${text} truncate min-w-0 flex-1 text-left`}>
                {entry.title || "Untitled"}
              </span>
              {showWorkspace && entry.workspace_name && (
                <span className={`${metaText} shrink-0 text-[var(--ct-muted)] truncate max-w-[60px]`}>
                  {entry.workspace_name}
                </span>
              )}
              {entry.model && (
                <span className={`${metaText} shrink-0 px-1 rounded bg-[var(--ct-active)] text-[var(--ct-muted)]`}>
                  {entry.model}
                </span>
              )}
            </button>
          ))}
        </div>
      )}
      {tooltip && createPortal(
        <div
          className="fixed z-[9999] pointer-events-none px-2 py-1 rounded text-[11px] text-[var(--ct-fg)] bg-[var(--ct-surface)] border border-[var(--ct-border)] shadow-lg max-w-[240px] break-words"
          style={{ left: tooltip.x + 14, top: tooltip.y - 8 }}
        >
          {tooltip.text}
        </div>,
        document.body,
      )}
    </div>
  );
}

export const RecentlyClosed = memo(RecentlyClosedInner);
