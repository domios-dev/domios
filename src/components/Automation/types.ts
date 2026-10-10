export interface AutomationScope {
  workspace_id: string;
  team_id: string | null;
}

export const ALL_JOBS_SCOPE = "__all_jobs__";

export interface AutomationIdentity extends AutomationScope {
  kind: "blueprint" | "cron" | "worker";
  id: string;
  owner_tab_id: string | null;
}

export type CommandResultStatus = "success" | "nonzero" | "error" | "cancelled" | "deadline" | "unconfirmed";

export interface CommandCapture {
  total_bytes: number;
  retained_bytes: number;
  truncated: boolean;
}

// The wire also carries primary/cleanup diagnostics; they are unscrubbed, so the UI type omits them.
export interface CommandResultMetadata {
  status: CommandResultStatus;
  exit_code: number | null;
  resources_confirmed: boolean;
  stdout: CommandCapture | null;
  stderr: CommandCapture | null;
  started_at: number;
  finished_at: number;
}

export interface AutomationHandlerResult {
  status: string | null;
  execution_id: string | null;
  accepted_at: number | null;
  command_result?: CommandResultMetadata | null;
}

export interface AutomationHandler {
  id: string;
  type: string;
  label?: string;
  timeout_ms?: number;
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

export interface AgentBlueprintDefinition {
  definition_version?: number;
  source: "stored_agent_definition";
  definition_id: string;
  creator_tab_id: string;
}

export interface AutomationNode {
  conversation?: import("../../lib/blueprintRun").ConversationCounts;
  source?: { type: string; kind?: string; display_name?: string; ref_type?: string; convert_to?: string | null; pattern?: string | null; idle_ms?: number | null };
  detail?: {
    observe_target?: { kind: string; id: string } | null;
    definition?: {
      unavailable?: string;
      command_language?: string;
      decision_kind?: "pattern" | "agent";
      question?: string;
      buckets?: readonly { name: string; pattern: string | null }[];
      probe?: string | null;
      probe_truncated?: boolean;
      commands?: readonly { bucket: string; handler_id: string; label: string; command: string; command_truncated: boolean; timeout_ms: number }[];
    };
  };
  history?: readonly AutomationTransition[];
  history_truncated?: boolean;
  blueprint_definition?: AgentBlueprintDefinition | null;
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

export interface CausalRunContext {
  id: string;
  root_node: string;
  root_transition: string;
  started_at: number;
  parent: { node: string; transition: string; handler: string } | null;
}

export interface AutomationRun {
  id: string;
  root_node: string;
  root_transition: string;
  started_at: number;
  state: "running" | "completed" | "unconfirmed";
  incomplete: boolean;
  transitions: readonly { node: AutomationIdentity; transition_id: string; parent: CausalRunContext["parent"] }[];
}

export interface AutomationTransition {
  decided_by?: "adapter" | "pattern" | null;
  adapter_id?: string | null;
  model?: string | null;
  confidence?: number | null;
  reason?: string | null;
  causal_run?: CausalRunContext | null;
  routes_current?: boolean;
  id: string;
  bucket: string;
  accepted_at: number;
  handlers: readonly (AutomationHandlerResult & { handler_id: string; summary?: string | null })[];
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

export interface AutomationScenario {
  id: string;
  name: string;
  root: AutomationIdentity;
}

export interface AutomationGraph extends AutomationScope {
  configured_fallback?: { adapter_id: string; model: string | null } | null;
  all_teams?: boolean;
  truncated?: boolean;
  scenarios: readonly AutomationScenario[];
  revision: string;
  nodes: readonly AutomationNode[];
  edges: readonly AutomationEdge[];
  blueprints?: readonly BlueprintSnapshot[];
  runs?: readonly AutomationRun[];
}

export type BlueprintOwnerKind = "tab" | "team";
export type BlueprintState = "idle" | "running" | "held" | "stopped" | "failed";
export type BlueprintStepState = "waiting" | "running" | "done" | "failed" | "skipped";
export type BlueprintResultState = "waiting" | "delivering" | "delivered" | "failed";

export interface BlueprintOwner {
  kind: BlueprintOwnerKind;
  tab_id: string | null;
  team_id: string | null;
}

export interface BlueprintStep {
  index: number;
  name: string;
  kind: import("./blueprintKinds").BlueprintStepKind;
  title: string;
  state: BlueprintStepState;
  outcome: string | null;
  started_at_ms: number | null;
  ended_at_ms: number | null;
  result_summary: string | null;
  reason: string | null;
}

export interface BlueprintResult {
  target: { kind: "owner_tab" | "tab" | "team" | "notify"; id: string | null };
  state: BlueprintResultState;
  reason: string | null;
  body_ref: string | null;
}

export interface BlueprintRunSnapshot {
  id: string;
  state: "idle" | "running" | "completed" | "failed" | "held";
  closed: boolean;
  started_at_ms: number;
  last_event_at_ms: number | null;
  current_step: number | null;
  steps: readonly BlueprintStep[];
  result: BlueprintResult;
}

export interface BlueprintSnapshot {
  runs?: readonly BlueprintRunSnapshot[];
  id: string;
  name: string;
  owner?: BlueprintOwner;
  state: BlueprintState;
  run_id: string | null;
  current_step: number | null;
  last_event_at_ms: number | null;
  trigger?: { type: "schedule" | "event" | "input"; summary: string };
  steps?: readonly BlueprintStep[];
  result?: BlueprintResult;
}

export interface AutomationAttachment extends AutomationScope {
  connection_id: string;
}

export interface AutomationGraphAdapter {
  read(attachment: AutomationAttachment, signal: AbortSignal): Promise<AutomationGraph>;
  subscribe(attachment: AutomationAttachment, changed: () => void): Promise<() => void>;
  stop(attachment: AutomationAttachment, identity: AutomationIdentity): Promise<void>;
  delete(attachment: AutomationAttachment, identity: AutomationIdentity): Promise<void>;
}

export function automationKey(identity: AutomationIdentity): string {
  return JSON.stringify([identity.workspace_id, identity.team_id, identity.kind, identity.id, identity.owner_tab_id]);
}

export function sameScope(a: AutomationScope, b: AutomationScope): boolean {
  return a.workspace_id === b.workspace_id && a.team_id === b.team_id;
}

export function automationKindLabel(kind: string): string {
  return kind === "blueprint" ? "node" : kind;
}
