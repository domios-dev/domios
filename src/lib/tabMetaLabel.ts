import type { ProviderDescriptor, Tab } from "./types";

export function tabMetaParts(model: string | null | undefined, fast: boolean | undefined, role?: string | null, status?: string | null): string[] {
  return [model, fast === true ? "fast" : null, role, status].filter((part): part is string => Boolean(part));
}

export function confirmedTabFastMode(tab: Tab, descriptor: ProviderDescriptor | undefined): boolean | undefined {
  if (!descriptor?.agent_runtime?.fastMode) return undefined;
  const confirmed = tab.agent_runtime_config?.confirmedFastMode;
  if (!confirmed || confirmed.sessionId !== tab.agent_session_id || !confirmed.bindingNonce
    || confirmed.bindingNonce !== tab.session_binding_nonce) return undefined;
  return confirmed.enabled;
}
