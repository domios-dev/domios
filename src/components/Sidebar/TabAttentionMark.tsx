import { TabLoadingIndicator } from "../shared/TabLoadingIndicator";
import type { TabAttention } from "./tabAttention";
import { tabStatusMark } from "./tabAttention";
import { BusyState } from "../shared/BusyState";

export function TabAttentionMark({ title, attention, active = false, placement = "icon" }: { title: string; attention?: TabAttention | null; active?: boolean; placement?: "icon" | "title" | "compact" }) {
  const mark = tabStatusMark(title, attention, active);
  if (!mark) return null;
  if (placement === "title" && mark.kind !== "starting") return null;
  if (placement === "icon" && mark.kind === "starting") return null;
  if (mark.kind === "starting") return <BusyState label={mark.label} inline hideLabel className="tab-starting-mark" />;
  return mark.kind !== "attention"
    ? <TabLoadingIndicator role="img" label={mark.label} className="tab-waiting-mark" compact deferred={false} />
    : <span role="img" aria-label={mark.label} title={mark.label} className="tab-attention-mark" data-kind={mark.level} />;
}
