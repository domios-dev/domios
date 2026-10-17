import type { RuntimeServiceTier, RuntimeServiceTierSelection } from "../../../packages/plugin-sdk/src/agentRuntime";

export function serviceTierOptions(declared: readonly RuntimeServiceTier[], available: readonly RuntimeServiceTier[] | undefined) {
  const tiers = available ?? declared;
  return [
    {value:JSON.stringify(null),label:"Default",title:"Standard speed",disabled:false},
    ...tiers.map(tier => ({value:JSON.stringify(tier.id),label:tier.name,title:tier.unavailableReason ?? tier.description,disabled:!!tier.unavailableReason})),
  ];
}

export function tierAfterModelChange(selection: RuntimeServiceTierSelection | undefined, available: readonly RuntimeServiceTier[] | undefined): RuntimeServiceTierSelection | undefined {
  return selection?.id && !available?.some(tier => tier.id === selection.id && !tier.unavailableReason) ? {id:null} : selection;
}
