import { Segmented, type SegmentedOption } from "../shared/Segmented";
import {
  PLAN_VIEW_MODES,
  planViewModeLabel,
  type PlanViewMode,
} from "./planViewMode";

const OPTIONS: SegmentedOption<PlanViewMode>[] = PLAN_VIEW_MODES.map((value) => ({
  value,
  label: planViewModeLabel(value),
}));

export function PlanViewModeSwitch({
  value,
  onChange,
}: {
  value: PlanViewMode;
  onChange: (mode: PlanViewMode) => void;
}) {
  return (
    <Segmented
      data-testid="plan-view-mode-switch"
      value={value}
      options={OPTIONS}
      height={26}
      onChange={onChange}
    />
  );
}
