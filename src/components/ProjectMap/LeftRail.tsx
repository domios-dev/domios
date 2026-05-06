import { useState } from "react";
import { ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight, Bookmark, BookmarkPlus } from "lucide-react";
import { NODE_KINDS, colorForKind } from "./styles";

interface Props {
  visibleKinds: Set<string>;
  toggleKind: (kind: string) => void;
  back: () => void;
  forward: () => void;
  canBack: boolean;
  canForward: boolean;
  bookmarks: number[];
  addBookmark: () => void;
  removeBookmark: (id: number) => void;
  jumpToBookmark: (id: number) => void;
  bookmarkLabel: (id: number) => string;
}

export function LeftRail(props: Props) {
  const [collapsed, setCollapsed] = useState(false);

  if (collapsed) {
    return (
      <div className="w-7 shrink-0 border-r border-[var(--ct-border)] bg-[var(--ct-bg)] flex flex-col items-center py-2">
        <button
          onClick={() => setCollapsed(false)}
          className="text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] p-1"
          title="Expand left rail"
        >
          <ChevronsRight size={12} />
        </button>
      </div>
    );
  }

  return (
    <div className="w-44 shrink-0 border-r border-[var(--ct-border)] bg-[var(--ct-bg)] flex flex-col text-[11px] text-[var(--ct-fg)]">
      <div className="flex items-center justify-between px-2 py-1.5 border-b border-[var(--ct-border)]">
        <span className="text-[10px] uppercase tracking-wide text-[var(--ct-muted)]">
          Filters
        </span>
        <button
          onClick={() => setCollapsed(true)}
          className="text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] p-0.5"
          title="Collapse"
        >
          <ChevronsLeft size={12} />
        </button>
      </div>

      <div className="px-2 py-2 flex flex-col gap-1">
        {NODE_KINDS.map((kind) => {
          const active = props.visibleKinds.size === 0 || props.visibleKinds.has(kind);
          return (
            <button
              key={kind}
              onClick={() => props.toggleKind(kind)}
              className={`flex items-center gap-2 px-2 py-1 rounded-sm text-left transition-colors ${
                active
                  ? "bg-[var(--ct-hover)] text-[var(--ct-fg)]"
                  : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)]"
              }`}
            >
              <span
                className="w-1.5 h-1.5 rounded-full"
                style={{ background: colorForKind(kind) }}
              />
              <span className="lowercase">{kind}</span>
            </button>
          );
        })}
      </div>

      <div className="border-t border-[var(--ct-border)] px-2 py-2">
        <div className="text-[10px] uppercase tracking-wide text-[var(--ct-muted)] mb-1">
          History
        </div>
        <div className="flex gap-1">
          <button
            onClick={props.back}
            disabled={!props.canBack}
            className="flex items-center gap-1 px-2 py-1 rounded-sm border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
            title="Back (Z)"
          >
            <ChevronLeft size={11} />
            <span>Z</span>
          </button>
          <button
            onClick={props.forward}
            disabled={!props.canForward}
            className="flex items-center gap-1 px-2 py-1 rounded-sm border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
            title="Forward (Shift+Z)"
          >
            <span>⇧Z</span>
            <ChevronRight size={11} />
          </button>
        </div>
      </div>

      <div className="border-t border-[var(--ct-border)] px-2 py-2 flex-1 min-h-0 overflow-y-auto">
        <div className="flex items-center justify-between mb-1">
          <span className="text-[10px] uppercase tracking-wide text-[var(--ct-muted)]">
            Bookmarks
          </span>
          <button
            onClick={props.addBookmark}
            title="Bookmark current focus"
            className="text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] p-0.5"
          >
            <BookmarkPlus size={11} />
          </button>
        </div>
        {props.bookmarks.length === 0 && (
          <div className="text-[10px] text-[var(--ct-muted)] italic px-1">none</div>
        )}
        {props.bookmarks.map((id) => (
          <div key={id} className="flex items-center gap-1 group/bm">
            <button
              onClick={() => props.jumpToBookmark(id)}
              className="flex-1 text-left px-1 py-0.5 rounded-sm text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] truncate"
              title={props.bookmarkLabel(id)}
            >
              <Bookmark size={10} className="inline mr-1 text-[var(--ct-yellow)]" />
              {props.bookmarkLabel(id)}
            </button>
            <button
              onClick={() => props.removeBookmark(id)}
              className="opacity-0 group-hover/bm:opacity-60 hover:!opacity-100 text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] px-1"
            >
              ×
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
