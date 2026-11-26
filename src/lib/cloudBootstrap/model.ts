import type { AgentRuntimeState } from "../agentRuntimeState";
import type {
  CloudBootstrapSnapshot,
  CloudCreatePhases,
  PrepareProviderSignInItem,
  ProviderSignInCardStatus,
} from "./types";

export const WARM_BOOTSTRAP_DRAFT_BUDGET_SECS = 10.0;

/**
 * Red-line invariant: a warm first console is never considered ready unless the target
 * verified a readable owner-context result after seeding (or preserved a verified restore).
 */
export function isWarmConsoleReady(snapshot: CloudBootstrapSnapshot | null | undefined): boolean {
  const warm = snapshot?.warm_bootstrap;
  if (!warm) return false;
  if (!warm.readable_owner_context || !warm.wake_up_verified) return false;
  return (
    warm.status === "applied" ||
    warm.status === "partial" ||
    warm.status === "already_applied" ||
    warm.status === "skipped_restored_state"
  );
}

export interface PartitionedProviderCards {
  signInRequired: ProviderSignInCardStatus[];
  ready: ProviderSignInCardStatus[];
  unsupported: ProviderSignInCardStatus[];
}

export function partitionProviderCards(
  providers: readonly ProviderSignInCardStatus[],
): PartitionedProviderCards {
  const signInRequired: ProviderSignInCardStatus[] = [];
  const ready: ProviderSignInCardStatus[] = [];
  const unsupported: ProviderSignInCardStatus[] = [];
  for (const item of providers) {
    if (item.state === "sign_in_required") signInRequired.push(item);
    else if (item.state === "ready") ready.push(item);
    else unsupported.push(item);
  }
  return { signInRequired, ready, unsupported };
}

export function providerCardKey(provider: Pick<ProviderSignInCardStatus, "provider_id" | "profile_id">): string {
  return provider.profile_id ? `${provider.provider_id}:${provider.profile_id}` : provider.provider_id;
}

/**
 * Builds the exact `AgentRuntimeState` shape consumed by Domios's existing `RuntimeLoginActions`
 * card so cloud provider sign-in reuses the single owner sign-in stack with zero duplication.
 */
export function buildRemoteRuntimeLoginState(
  provider: Pick<ProviderSignInCardStatus, "display_name" | "supported_methods" | "code_input" | "state">,
): AgentRuntimeState {
  if (provider.state === "ready") {
    return {
      state: "loaded",
      epoch: 0,
      auth: "signedIn",
      loginProviderName: provider.display_name,
    };
  }
  const loginMethods = provider.supported_methods.filter(
    (m): m is "browser" | "device" | "apiKey" =>
      m === "browser" || m === "device" || m === "apiKey",
  );
  return {
    state: "loginPending",
    epoch: 0,
    auth: "signedOut",
    loginProviderName: provider.display_name,
    loginCodeInput: provider.code_input,
    loginMethods: loginMethods.length > 0 ? loginMethods : ["browser"],
  };
}

export function resolveRemoteLoginTabId(
  peerId: string,
  provider: Pick<ProviderSignInCardStatus, "login_tab_id">,
  prepared?: PrepareProviderSignInItem | null,
): string | null {
  if (prepared?.remote_tab_id) return prepared.remote_tab_id;
  const tabId = prepared?.tab_id ?? provider.login_tab_id;
  if (!tabId || !peerId) return null;
  return tabId.startsWith("mesh:") ? tabId : `mesh:${peerId}/${tabId}`;
}

export interface PhaseTimingSummary {
  peerReadyDeltaSecs: number | null;
  warmBootstrapDeltaSecs: number | null;
  withinBudget: boolean | null;
  providerSignInSecs: number | null;
}

export function summarizeCreatePhases(
  phases: CloudCreatePhases | null | undefined,
): PhaseTimingSummary {
  if (!phases) {
    return {
      peerReadyDeltaSecs: null,
      warmBootstrapDeltaSecs: null,
      withinBudget: null,
      providerSignInSecs: null,
    };
  }
  const peerReadyDeltaSecs =
    phases.requested_at_ms != null &&
    phases.peer_ready_at_ms != null &&
    phases.peer_ready_at_ms >= phases.requested_at_ms
      ? (phases.peer_ready_at_ms - phases.requested_at_ms) / 1000
      : null;

  // Authoritative backend timestamps only: never fabricate warm_bootstrap_delta_secs from client wall clock alone.
  const warmBootstrapDeltaSecs =
    phases.warm_bootstrap_delta_secs != null
      ? phases.warm_bootstrap_delta_secs
      : phases.authoritative_backend_phases &&
          phases.seed_started_at_ms != null &&
          phases.warm_console_ready_at_ms != null &&
          phases.warm_console_ready_at_ms >= phases.seed_started_at_ms
        ? (phases.warm_console_ready_at_ms - phases.seed_started_at_ms) / 1000
        : null;

  const withinBudget =
    phases.within_budget != null
      ? phases.within_budget
      : warmBootstrapDeltaSecs != null
        ? warmBootstrapDeltaSecs <= WARM_BOOTSTRAP_DRAFT_BUDGET_SECS
        : null;

  const providerSignInSecs =
    phases.provider_signin_started_at_ms != null &&
    phases.provider_signin_completed_at_ms != null &&
    phases.provider_signin_completed_at_ms >= phases.provider_signin_started_at_ms
      ? (phases.provider_signin_completed_at_ms - phases.provider_signin_started_at_ms) / 1000
      : null;

  return {
    peerReadyDeltaSecs,
    warmBootstrapDeltaSecs,
    withinBudget,
    providerSignInSecs,
  };
}
