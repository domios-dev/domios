import { tabActivity } from "../../lib/tabActivity";
import type { ProviderDescriptor, Tab, TabStatus } from "../../lib/types";

export interface TabProviderMenuView {
  providers: ProviderDescriptor[];
  currentProviderId: string;
  blockedReason: string | null;
}

export function supportsProviderHandoff(provider: ProviderDescriptor): boolean {
  return provider.transport === "pty" || provider.agent_runtime != null;
}

export function tabProviderMenuView(tab: Tab, descriptors: ProviderDescriptor[], status: TabStatus | null | undefined): TabProviderMenuView | null {
  const current = tab.agent_type;
  if (!current) return null;
  const providers = descriptors.filter(supportsProviderHandoff);
  if (!providers.some((provider) => provider.id === current) || providers.length < 2) return null;
  return { providers, currentProviderId: current, blockedReason: tabActivity(tab, status).active ? "Wait for idle or stop the current turn before switching." : null };
}

export function tabProviderEntryLabel(_view: TabProviderMenuView): string {
  return "Switch harness…";
}

export interface ProviderSwitchOptions { model?: string; reasoningEffort?: string; force?: boolean }
export type ProviderSwitchResult = import("../../../gen/ProviderSwitchOutcome").ProviderSwitchOutcome;

export type ProviderSwitchStatus = import("../../../gen/ProviderSwitchStatus").ProviderSwitchStatus;

export interface TabProviderSwitchApi {
  switchProvider(tabId: string, providerId: string, options?: ProviderSwitchOptions): Promise<ProviderSwitchStatus | ProviderSwitchResult | void>;
}

export async function switchTabProvider(api: TabProviderSwitchApi, tabId: string, providerId: string, options?: ProviderSwitchOptions): Promise<ProviderSwitchStatus | ProviderSwitchResult | void> {
  return api.switchProvider(tabId, providerId, options);
}
