import { useMemo } from "react";
import { Map } from "lucide-react";
import { useMultiplexer } from "../../store/multiplexer";
import { usePlanIndexOptional } from "../Canvas/PlansProvider";
import { findPlanIndexForPane } from "../Canvas/planPaneMatch";
import { openPanePlanFocus } from "../Canvas/workspacePlanEntry";
import { PANE_BAR_CONTROL_H } from "./paneBarConstants";

interface PanePlanFocusChipProps {
  paneId: string;
}

export function PanePlanFocusChip({ paneId }: PanePlanFocusChipProps) {
  const { state } = useMultiplexer();
  const { index } = usePlanIndexOptional();
  const entries = useMemo(
    () => findPlanIndexForPane(index, paneId, state.panes),
    [index, paneId, state.panes],
  );
  const pane = state.panes[paneId];
  const label = useMemo(() => {
    if (pane?.description?.trim()) return pane.description.trim();
    if (entries.length === 1) return entries[0].title.trim() || "Plan";
    if (entries.length > 1) return `Plans (${entries.length})`;
    return null;
  }, [pane?.description, entries]);

  if (!label || entries.length === 0) return null;

  return (
    <button
      type="button"
      data-testid="pane-plan-focus-chip"
      title="Open plan canvas and focus this pane's items"
      onClick={() => openPanePlanFocus(entries, paneId)}
      className="flex max-w-[min(240px,42vw)] shrink items-center gap-1 rounded border px-2 text-[10px] font-medium cursor-pointer truncate"
      style={{
        height: PANE_BAR_CONTROL_H,
        background: "color-mix(in srgb, var(--ct-accent) 10%, var(--ct-bg))",
        borderColor: "color-mix(in srgb, var(--ct-accent) 35%, var(--ct-border))",
        color: "var(--ct-fg)",
      }}
    >
      <Map size={11} className="shrink-0 opacity-80" />
      <span className="truncate">{label}</span>
    </button>
  );
}
