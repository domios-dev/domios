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
  return "Switch provider…";
}

export interface ProviderSwitchOptions { model?: string; reasoningEffort?: string; force?: boolean }
export interface ProviderSwitchResult {
  tab_id: string;
  provider: string;
  session_id: string;
  previous_provider: string;
  previous_session_id: string | null;
  previous_history_path: string;
  binding_restored: boolean;
  handoff_delivered: boolean;
}

export interface ProviderSwitchStatus { operation_id: string; tab_id: string; provider: string; phase: "pending" | "running" | "completed" | "failed"; outcome: ProviderSwitchResult | null; error: string | null }

export interface TabProviderSwitchApi {
  switchProvider(tabId: string, providerId: string, options?: ProviderSwitchOptions): Promise<ProviderSwitchStatus | ProviderSwitchResult | void>;
}

export async function switchTabProvider(api: TabProviderSwitchApi, tabId: string, providerId: string, options?: ProviderSwitchOptions): Promise<ProviderSwitchStatus | ProviderSwitchResult | void> {
  return api.switchProvider(tabId, providerId, options);
}
