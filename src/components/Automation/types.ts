export interface AutomationScope {
  workspace_id: string;
  team_id: string | null;
}

export interface AutomationIdentity extends AutomationScope {
  kind: "sentinel" | "cron" | "worker";
  id: string;
  owner_tab_id: string | null;
}

export interface AutomationHandlerResult {
  status: string | null;
  execution_id: string | null;
  accepted_at: number | null;
}

export interface AutomationHandler {
  id: string;
  type: string;
  target: AutomationIdentity | null;
  availability: "available" | "missing" | "unavailable";
  last_result: AutomationHandlerResult | null;
}

export interface AutomationOutcome {
  bucket: string;
  handlers: readonly AutomationHandler[];
}

export interface AutomationExecutor {
  kind: string | null;
  provider_id: string | null;
  adapter_id: string | null;
  model: string | null;
  capabilities: readonly string[] | null;
}

export interface AutomationNode {
  executor?: AutomationExecutor | null;
  identity: AutomationIdentity;
  name: string;
  purpose: string | null;
  state: string | null;
  availability: "available" | "missing" | "unavailable";
  hidden: boolean | null;
  schedule: string | null;
  cadence_ms: number | null;
  expires_at: number | null;
  outcomes: readonly AutomationOutcome[];
  last_result: { bucket: string; accepted_at: number; handlers: readonly (AutomationHandlerResult & { handler_id: string })[] } | null;
  stop: { allowed: boolean; reason: string | null };
}

export interface AutomationEdge {
  bucket: string;
  handler_id: string;
  handler_type: string;
  result: AutomationHandlerResult | null;
  id: string;
  source: AutomationIdentity;
  target: AutomationIdentity;
  evidence: { kind: "configured"; route_id: string } | { kind: "observed"; execution_id: string };
}

export interface AutomationGraph extends AutomationScope {
  revision: string;
  nodes: readonly AutomationNode[];
  edges: readonly AutomationEdge[];
}

export interface AutomationAttachment extends AutomationScope {
  connection_id: string;
}

export interface AutomationGraphAdapter {
  read(attachment: AutomationAttachment, signal: AbortSignal): Promise<AutomationGraph>;
  subscribe(attachment: AutomationAttachment, changed: () => void): Promise<() => void>;
  stop(attachment: AutomationAttachment, identity: AutomationIdentity): Promise<void>;
}

export function automationKey(identity: AutomationIdentity): string {
  return JSON.stringify([identity.workspace_id, identity.team_id, identity.kind, identity.id, identity.owner_tab_id]);
}

export function sameScope(a: AutomationScope, b: AutomationScope): boolean {
  return a.workspace_id === b.workspace_id && a.team_id === b.team_id;
}
