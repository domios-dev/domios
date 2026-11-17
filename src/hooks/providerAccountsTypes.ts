import type { AgentRuntimeState } from "../lib/agentRuntimeState";

export interface AccountLoginContext {
  tabId: string;
  state: AgentRuntimeState;
}

export interface ProviderAccountRow {
  id: string;
  name?: string;
  identity?: string;
  plan?: string;
  status: "ready" | "signedOut" | "signingIn" | "exhausted" | "removing" | "checking" | "probeFailed" | "unknown";
  probeFailureReason?: string;
  removalNotice?: { id: string; kind: "inUse" };
  tabsInUse: number;
  tabsToFinish?: number;
  resetsAtMs?: number;
  login?: AccountLoginContext;
}

export interface ProviderAccountsSnapshot {
  providerId: string;
  revision: string;
  signInId?: string;
  accounts: ProviderAccountRow[];
  automaticSwitch: boolean;
  continuation: "unverified" | "unsupported" | "proven";
  workspaceOverrides: { workspaceId: string; workspaceName: string; accountId: string }[];
  adding?: {
    identity?: string;
    error?: string;
    details?: string;
  } & ({ status: "signingIn"; context: AccountLoginContext } | { status: "duplicate" | "failed"; context?: AccountLoginContext });
  /** Positions are zero-based in requests and feedback. */
  feedback?: { id: string; kind: "added" | "moved"; accountId: string; position: number };
}

export type ProviderAccountsAction =
  | { action: "add" | "retryAdd" | "cancelAdd" }
  | { action: "signIn" | "cancelSignIn" | "remove" | "check"; accountId: string }
  | { action: "rename"; accountId: string; name: string }
  | { action: "reorder"; accountId: string; position: number }
  | { action: "automaticSwitch"; enabled: boolean }
  | { action: "resetWorkspace"; workspaceId: string }
  | { action: "workspace"; workspaceId: string; accountId: string | null }
  | { action: "dismissRemovalNotice"; accountId: string; noticeId: string };
