import type { BlueprintProjectionInput } from "../../lib/blueprintProjectionView";

type OptionalFields<T, K extends keyof T> = Omit<T, K> & Partial<Pick<T, K>>;
type UnionKeys<T> = T extends unknown ? keyof T : never;
type UnionFields<T> = { [K in UnionKeys<T>]?: T extends unknown ? K extends keyof T ? T[K] : never : never };
type WireNode = import("../../../gen/AutomationNode").AutomationNode;
type WireTransition = import("../../../gen/AutomationTransition").AutomationTransition;
type WireGraph = import("../../../gen/AutomationGraphSnapshot").AutomationGraphSnapshot;
type SafeTransitionHandler = Omit<import("../../../gen/AutomationTransitionHandler").AutomationTransitionHandler, "command_result"> & { command_result?: CommandResultMetadata | null };
type NodeDetail = Omit<Partial<import("../../../gen/AutomationDetail").AutomationDetail>, "definition"> & { definition?: UnionFields<import("../../../gen/AutomationDefinition").AutomationDefinition> };
type NodeLastResult = Omit<OptionalFields<import("../../../gen/AutomationLastResult").AutomationLastResult, "decided_by" | "adapter_id" | "model" | "confidence">, "handlers"> & { handlers: readonly SafeTransitionHandler[] };

export interface AutomationScope {
  workspace_id: string;
  team_id: string | null;
}

export const ALL_JOBS_SCOPE = "__all_jobs__";

export type AutomationIdentity = import("../../../gen/AutomationIdentity").AutomationIdentity;

export type CommandResultStatus = import("../../../gen/SentinelCommandStatus").SentinelCommandStatus;

export type CommandCapture = import("../../../gen/SentinelCommandCapture").SentinelCommandCapture;

// The wire also carries primary/cleanup diagnostics; they are unscrubbed, so the UI type omits them.
export type CommandResultMetadata = Pick<import("../../../gen/SentinelCommandResultMetadata").SentinelCommandResultMetadata, "status" | "exit_code" | "resources_confirmed" | "stdout" | "stderr" | "started_at" | "finished_at">;

export type AutomationHandlerResult = Omit<import("../../../gen/AutomationHandlerResult").AutomationHandlerResult, "command_result"> & { command_result?: CommandResultMetadata | null };

export type AutomationHandler = Omit<import("../../../gen/AutomationHandler").AutomationHandler, "last_result"> & { last_result: AutomationHandlerResult | null };

export type AutomationOutcome = Omit<import("../../../gen/AutomationOutcome").AutomationOutcome, "handlers"> & { handlers: readonly AutomationHandler[] };

export type AutomationExecutor = OptionalFields<import("../../../gen/AutomationExecutor").AutomationExecutor, "selection" | "configured_model" | "reported_model">;

export type AgentBlueprintDefinition = OptionalFields<import("../../../gen/AgentBlueprintDefinition").AgentBlueprintDefinition, "lineage" | "definition_version">;

export type AutomationNode = Omit<WireNode, "detail" | "outcomes" | "last_result" | "executor" | "blueprint_definition" | "history" | "source"> & {
  detail?: NodeDetail;
  outcomes: readonly AutomationOutcome[];
  last_result: NodeLastResult | null;
  executor?: AutomationExecutor | null;
  blueprint_definition?: AgentBlueprintDefinition | null;
  history?: readonly AutomationTransition[];
  source?: Pick<import("../../../gen/AutomationSourceDescriptor").AutomationSourceDescriptor, "type"> & UnionFields<import("../../../gen/AutomationSourceDescriptor").AutomationSourceDescriptor>;
};

export type CausalRunContext = import("../../../gen/CausalRunContext").CausalRunContext;

export type AutomationRun = Omit<import("../../../gen/AutomationRun").AutomationRun, "transitions"> & { transitions: readonly OptionalFields<import("../../../gen/AutomationRunTransition").AutomationRunTransition, "conversation">[] };

export type AutomationTransition = Omit<OptionalFields<WireTransition, "causal_run" | "routes_current" | "pending_handlers" | "decided_by" | "adapter_id" | "model" | "confidence">, "handlers"> & { handlers: readonly SafeTransitionHandler[] };

export type AutomationEdge = Omit<import("../../../gen/AutomationEdge").AutomationEdge, "result"> & { result: AutomationHandlerResult | null };

export type AutomationScenario = import("../../../gen/AutomationScenario").AutomationScenario;

export type AutomationGraph = Pick<WireGraph, "workspace_id" | "team_id" | "revision"> & Partial<Omit<WireGraph, "workspace_id" | "team_id" | "revision" | "nodes" | "edges" | "scenarios" | "blueprints" | "runs" | "quarantined">> & {
  scenarios: readonly AutomationScenario[]; nodes: readonly AutomationNode[]; edges: readonly AutomationEdge[];
  blueprints?: readonly BlueprintSnapshot[]; runs?: readonly AutomationRun[]; quarantined?: readonly QuarantinedBlueprint[];
};

/** A committed Blueprint refused at load; it never runs until recreated. */
export type QuarantinedBlueprint = OptionalFields<import("../../../gen/QuarantinedBlueprint").QuarantinedBlueprint, "scenario_id" | "workspace_id" | "team_id" | "creator_tab_id">;

export type BlueprintOwnerKind = import("../../../gen/BlueprintOwner").BlueprintOwner;
export type BlueprintState = import("../../../gen/BlueprintState").BlueprintState;
export type BlueprintStepState = import("../../../gen/BlueprintStepState").BlueprintStepState;
export type BlueprintResultState = import("../../../gen/BlueprintResultState").BlueprintResultState;

export type BlueprintOwner = import("../../../gen/BlueprintSnapshotOwner").BlueprintSnapshotOwner;

export type BlueprintStep = import("../../../gen/BlueprintStep").BlueprintStep;

export type BlueprintResult = import("../../../gen/BlueprintSnapshotResult").BlueprintSnapshotResult;

export type BlueprintRunSnapshot = Omit<import("../../../gen/BlueprintRunSnapshot").BlueprintRunSnapshot, keyof BlueprintProjectionInput> & BlueprintProjectionInput;

export type BlueprintSnapshot = Omit<OptionalFields<import("../../../gen/BlueprintSnapshot").BlueprintSnapshot, "owner" | "runs" | "steps" | "note" | "stopped_at" | "deletes_at">, keyof BlueprintProjectionInput | "runs"> & BlueprintProjectionInput & { runs?: readonly BlueprintRunSnapshot[] };

export interface AutomationAttachment extends AutomationScope {
  connection_id: string;
}

export interface AutomationGraphAdapter {
  read(attachment: AutomationAttachment, signal: AbortSignal): Promise<AutomationGraph>;
  /** The last graph this page read for the attachment, so a remounted surface starts from it rather than from nothing. */
  peek?(attachment: AutomationAttachment): AutomationGraph | null;
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
