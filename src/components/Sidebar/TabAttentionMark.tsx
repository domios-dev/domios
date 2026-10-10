import type { TabAttention } from "./tabAttention";
import { tabAttentionLabel } from "./tabAttention";

export function TabAttentionMark({ title, attention, active = false }: { title: string; attention?: TabAttention | null; active?: boolean }) {
  if (!attention?.showMark || active) return null;
  const label = tabAttentionLabel(title, attention);
  return <span role="img" aria-label={label} title={label} className="tab-attention-mark" data-kind={attention.kind} />;
}
