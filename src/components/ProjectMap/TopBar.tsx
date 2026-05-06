import { RefreshCw, Loader2 } from "lucide-react";
import type { MapProgress } from "./hooks/useMapProgress";

interface Props {
  workspaceName: string;
  onRefresh: () => void;
  progress: MapProgress | null;
}

const PHASE_LABEL: Record<string, string> = {
  scan: "scanning",
  index: "indexing",
  graph: "computing graph",
  embed: "embedding",
  done: "done",
};

export function TopBar({ workspaceName, onRefresh, progress }: Props) {
  const showProgress = progress?.running ?? false;
  const pct = progress && progress.total > 0
    ? Math.min(100, Math.round((progress.done / progress.total) * 100))
    : null;

  return (
    <div className="flex items-center gap-2 px-3 h-9 border-b border-[var(--ct-border)] bg-[var(--ct-bg)] text-xs text-[var(--ct-fg)]">
      <span className="px-2 py-0.5 rounded-sm bg-[var(--ct-active)] text-[var(--ct-fg)] font-medium">
        {workspaceName}
      </span>
      <span className="text-[10px] uppercase tracking-wide text-[var(--ct-muted)]">
        Project Map
      </span>
      {showProgress && progress && (
        <div className="flex items-center gap-2 px-2 py-0.5 rounded-sm bg-[var(--ct-yellow)]/10 border border-[var(--ct-yellow)]/30 text-[10px] text-[var(--ct-yellow)]">
          <Loader2 size={10} className="animate-spin" />
          <span>{PHASE_LABEL[progress.phase] ?? progress.phase}</span>
          {progress.total > 0 && (
            <span className="font-mono">
              {progress.done}/{progress.total}
              {pct != null && ` · ${pct}%`}
            </span>
          )}
        </div>
      )}
      <div className="flex-1" />
      <button
        onClick={onRefresh}
        title="Refresh"
        className="flex items-center gap-1 px-2 h-6 rounded-sm text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] transition-colors"
      >
        <RefreshCw size={11} />
        <span className="text-[11px]">refresh</span>
      </button>
    </div>
  );
}
