import { tabActivity } from "../../lib/tabActivity";
/** Sidebar-row provider switch: which tabs offer it, what the entry says, and
 *  what changing it costs. Kept beside `tabModelMenu` because the two look alike
 *  in the menu and are not alike at all — one is an in-session command, this one
 *  relaunches the agent. */
import type { ProviderDescriptor, Tab, TabStatus } from "../../lib/types";

export interface TabProviderMenuView {
  /** PTY providers this tab could run, in catalog order. */
  providers: ProviderDescriptor[];
  currentProviderId: string;
  /** Non-null while a turn is in flight — switching would kill it. */
  blockedReason: string | null;
}

/**
 * The row's provider-switch surface, or null when this tab has no route.
 *
 * Offered for a PTY agent tab when the catalog holds another PTY provider to
 * switch to. chatBackend shells are excluded: their session is owned by the
 * plugin transport, not by a relaunchable PTY, so the same gesture would mean
 * something different there. Gated on `transport`, never on a provider id.
 */
export function tabProviderMenuView(
  tab: Tab,
  descriptors: ProviderDescriptor[],
  status: TabStatus | null | undefined,
): TabProviderMenuView | null {
  const current = tab.agent_type;
  if (!current || tab.backing?.kind === "chat_backend") return null;
  const providers = descriptors.filter((descriptor) => descriptor.transport === "pty");
  if (!providers.some((descriptor) => descriptor.id === current)) return null;
  if (providers.length < 2) return null;
  return {
    providers,
    currentProviderId: current,
    blockedReason:
      tabActivity(tab, status).active ? "Working — switching the provider would end this turn" : null,
  };
}

/** Accessible name of the entry that opens the picker. Derived from the
 *  descriptor so a renamed provider renames the control. */
export function tabProviderEntryLabel(view: TabProviderMenuView): string {
  const current = view.providers.find((descriptor) => descriptor.id === view.currentProviderId);
  return current ? `Provider: ${current.display_name}` : "Provider";
}

export interface TabProviderSwitchApi {
  restartWithProvider: (tabId: string, providerId: string) => Promise<void>;
}

export async function switchTabProvider(
  api: TabProviderSwitchApi,
  tabId: string,
  providerId: string,
): Promise<void> {
  await api.restartWithProvider(tabId, providerId);
}
