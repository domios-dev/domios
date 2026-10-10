import { TabLoadingIndicator } from "../shared/TabLoadingIndicator";
import type { TabAttention } from "./tabAttention";
import { tabStatusMark } from "./tabAttention";

export function TabAttentionMark({ title, attention, active = false }: { title: string; attention?: TabAttention | null; active?: boolean }) {
  const mark = tabStatusMark(title, attention, active);
  if (!mark) return null;
  return mark.kind === "starting"
    ? <TabLoadingIndicator role="img" label={mark.label} className="tab-starting-mark" compact deferred={false} />
    : <span role="img" aria-label={mark.label} title={mark.label} className="tab-attention-mark" data-kind={mark.level} />;
}
