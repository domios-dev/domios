import type { RuntimeServiceTier, RuntimeServiceTierSelection } from "../../../packages/plugin-sdk/src/agentRuntime";
import { Segmented } from "../shared/Segmented";
import { serviceTierOptions } from "./serviceTierPickerModel";

export function ServiceTierPicker({declared,available,selection,busy,onChange}:{
  declared:readonly RuntimeServiceTier[];
  available:readonly RuntimeServiceTier[]|undefined;
  selection:RuntimeServiceTierSelection|undefined;
  busy:boolean;
  onChange:(selection:RuntimeServiceTierSelection)=>void;
}) {
  const options = serviceTierOptions(declared,available);
  const reasons = [...new Set(options.filter(option => option.disabled).map(option => option.title))];
  return <div className="ct-model-picker__effort">
    <span className="ct-model-picker__label">Speed</span>
    <Segmented aria-label="Speed" value={JSON.stringify(selection?.id ?? null)}
      options={options.map(option => ({...option,disabled:busy || option.disabled}))}
      onChange={value => { if (!busy && options.some(option => option.value === value && !option.disabled)) onChange({id:JSON.parse(value) as string|null}); }} />
    {reasons.length > 0 && <span className="text-xs text-[var(--ct-fg-muted)]">{reasons.join(" · ")}</span>}
  </div>;
}
