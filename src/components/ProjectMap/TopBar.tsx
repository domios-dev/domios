import { RefreshCw } from "lucide-react";

interface Props {
  workspaceName: string;
  layout: "force";
  onLayoutChange: (layout: "force") => void;
  onRefresh: () => void;
  loading: boolean;
}

export function TopBar({ workspaceName, layout, onLayoutChange, onRefresh, loading }: Props) {
  return (
    <div className="flex items-center gap-2 px-3 h-9 border-b border-[var(--ct-border)] bg-[var(--ct-bg)] text-xs text-[var(--ct-fg)]">
      <span className="px-2 py-0.5 rounded-sm bg-[var(--ct-active)] text-[var(--ct-fg)] font-medium">
        {workspaceName}
      </span>
      <span className="text-[10px] uppercase tracking-wide text-[var(--ct-muted)]">
        Project Map
      </span>
      <div className="flex-1" />
      <select
        value={layout}
        onChange={(e) => onLayoutChange(e.target.value as "force")}
        className="bg-[var(--ct-bg)] text-[var(--ct-fg)] border border-[var(--ct-border)] rounded-sm px-1 py-0.5 text-[11px] outline-none"
        title="Layout algorithm"
      >
        <option value="force">force</option>
      </select>
      <button
        onClick={onRefresh}
        title="Refresh graph"
        className="flex items-center gap-1 px-2 h-6 rounded-sm text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] transition-colors"
      >
        <RefreshCw size={11} className={loading ? "animate-spin" : undefined} />
        <span className="text-[11px]">refresh</span>
      </button>
    </div>
  );
}
