import { useRef, useState, type ReactNode } from "react";
import { useComposerHeight } from "./useComposerHeight";
import { Eye } from "lucide-react";
import type { WatcherSpec } from "../../lib/types";
import { charterPreview } from "../../lib/watcherUi";
import { WatcherStatusBadge } from "./WatcherStatusBadge";

interface WatcherReadOnlyBarProps {
  watcher: WatcherSpec;
  modelBubble?: ReactNode;
}

export function WatcherReadOnlyBar({ watcher, modelBubble }: WatcherReadOnlyBarProps) {
  const surfaceRef = useRef<HTMLDivElement>(null);
  useComposerHeight(surfaceRef);
  const [charterExpanded, setCharterExpanded] = useState(false);
  const charter = watcher.charter.trim();
  const canExpandCharter = charter.length > charterPreview(charter, false).length;

  return (
    <div
      ref={surfaceRef}
      data-testid="watcher-readonly-bar"
      className="ct-composer-readonly shrink-0 border-t py-2.5"
      style={{
        borderColor: "var(--ct-border, #30363d)",
        background: "color-mix(in srgb, var(--ct-bg, #0d1117) 94%, var(--ct-muted-fg, #8b949e) 6%)",
      }}
    >
      <div className="mx-auto flex w-full ct-composer-lane min-w-0 flex-col gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <Eye size={14} strokeWidth={2.2} style={{ color: "var(--ct-muted-fg, #8b949e)" }} />
          <span className="text-xs font-medium" style={{ color: "var(--ct-fg, #c9d1d9)" }}>
            Watcher — read-only
          </span>
          <WatcherStatusBadge lastStatus={watcher.last_status} />
        </div>
        {charter && (
          <div
            data-testid="watcher-charter-preview"
            className="min-w-0 rounded-md border px-2.5 py-2 text-[11px] leading-relaxed"
            style={{
              color: "var(--ct-muted-fg, #8b949e)",
              borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
              background: "color-mix(in srgb, var(--ct-surface, #161b22) 92%, transparent)",
            }}
          >
            <div className={charterExpanded ? "whitespace-pre-wrap break-words" : "truncate"}>
              {charterPreview(charter, charterExpanded)}
            </div>
            {canExpandCharter && (
              <button
                type="button"
                data-testid="watcher-charter-expand"
                onClick={() => setCharterExpanded((v) => !v)}
                className="mt-1 text-[10px] font-mono uppercase tracking-wide hover:brightness-125"
                style={{ color: "var(--ct-accent, #58a6ff)", background: "transparent", border: "none", padding: 0 }}
              >
                {charterExpanded ? "Show less" : "Show charter"}
              </button>
            )}
          </div>
        )}
        {modelBubble && (
          <div data-testid="watcher-readonly-bubbles" className="flex flex-wrap items-center gap-1">
            {modelBubble}
          </div>
        )}
      </div>
    </div>
  );
}
