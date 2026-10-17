import type { ProviderDescriptor, Tab } from "./types";
import type { RuntimeServiceTierSelection as ServiceTierSelection } from "../../packages/plugin-sdk/src/agentRuntime";

export function tabMetaParts(model: string | null | undefined, tierLabel: string | undefined, role?: string | null, status?: string | null): string[] {
  return [model, tierLabel, role, status].filter((part): part is string => Boolean(part));
}

export function confirmedTabServiceTier(tab: Tab, descriptor: ProviderDescriptor | undefined): ServiceTierSelection | undefined {
  if (!descriptor?.agent_runtime?.serviceTiers) return undefined;
  const confirmed = tab.agent_runtime_config?.confirmedServiceTier;
  if (!confirmed || confirmed.sessionId !== tab.agent_session_id || !confirmed.bindingNonce
    || confirmed.bindingNonce !== tab.session_binding_nonce) return undefined;
  return confirmed.selection;
}

export function serviceTierLabel(descriptor: ProviderDescriptor | undefined, selection: ServiceTierSelection | undefined): string | undefined {
  if (!selection?.id) return undefined;
  return (descriptor?.agent_runtime?.serviceTiers?.find(tier => tier.id === selection.id)?.name ?? selection.id).toLowerCase();
}

export function tabServiceTierAvailability(tab: Tab) {
  const binding = tab.agent_runtime_config?.serviceTierAvailabilityBinding;
  return binding?.sessionId === tab.agent_session_id && binding?.bindingNonce && binding.bindingNonce === tab.session_binding_nonce
    ? tab.agent_runtime_config?.serviceTierAvailability : undefined;
}
