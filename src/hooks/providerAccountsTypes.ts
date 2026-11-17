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
  status: "ready" | "signedOut" | "signingIn" | "exhausted" | "removing";
  tabsInUse: number;
  tabsToFinish?: number;
  resetsAtMs?: number;
  login?: AccountLoginContext;
}

export interface ProviderAccountsSnapshot {
  providerId: string;
  revision: string;
  accounts: ProviderAccountRow[];
  automaticSwitch: boolean;
  continuation: "unverified" | "unsupported" | "proven";
  workspaceOverrides: { workspaceId: string; workspaceName: string; accountId: string }[];
  adding?: {
    context: AccountLoginContext;
    status: "signingIn" | "duplicate" | "failed";
    identity?: string;
    error?: string;
    details?: string;
  };
  /** Positions are zero-based in requests and feedback. */
  feedback?: { id: string; kind: "added" | "moved"; accountId: string; position: number };
}

export type ProviderAccountsAction =
  | { action: "add" | "retryAdd" | "cancelAdd" }
  | { action: "signIn" | "cancelSignIn" | "remove"; accountId: string }
  | { action: "rename"; accountId: string; name: string }
  | { action: "reorder"; accountId: string; position: number }
  | { action: "automaticSwitch"; enabled: boolean }
  | { action: "resetWorkspace"; workspaceId: string };
