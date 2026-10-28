import type { NodeSchema } from "./capabilities";

export type BlueprintPortKind = "text" | "json" | "image" | "audio" | "video" | "file";

export interface BlueprintStepPort {
  kind: BlueprintPortKind;
  minItems: number;
  maxItems: number;
  schema?: NodeSchema;
}

export interface BlueprintStepDescriptor {
  contractVersion: 1;
  revision: number;
  id: string;
  title: string;
  description: string;
  params: NodeSchema;
  inputs: Record<string, BlueprintStepPort>;
  outputs: Record<string, BlueprintStepPort>;
  outcomes: string[];
  lifecycle: { reconcile: boolean; cancel: boolean };
  effects: { sideEffects: boolean; potentiallyPaid: boolean };
}

export interface BlueprintRunScope {
  workspace: string;
  team: string | null;
  run: string;
}

export interface BlueprintInputLineage {
  plugin: string;
  operation: string;
  port: string;
}

export interface BlueprintStepContext {
  scope: BlueprintRunScope;
  owner: string;
  node: string;
  attempt: string;
  operation: string;
  runView: string;
  runLabel: string;
  nodeLabel: string;
  inputLineage: BlueprintInputLineage[];
}

export type BlueprintPortValue =
  | { kind: "text"; text: string }
  | { kind: "json"; value: unknown }
  | { kind: "media"; handle: string; media: Exclude<BlueprintPortKind, "text" | "json"> };

export type BlueprintOutputValue = Exclude<BlueprintPortValue, { kind: "media" }>
  | { kind: "media_source"; id: string; media: Exclude<BlueprintPortKind, "text" | "json"> };

export type BlueprintPortValues = Record<string, BlueprintPortValue[]>;

export interface BlueprintStepRequest {
  context: BlueprintStepContext;
  nodeType: string;
  revision: number;
  params: Record<string, unknown>;
  inputs: BlueprintPortValues;
  unit: string;
  stepAllowance: number;
  job: string | null;
  deadlineMs: number;
}

export interface BlueprintStepEstimate {
  amount: number;
  unit: string;
  validUntilMs: number;
}

export type BlueprintOperationState = "prepared" | "waiting_for_owner_ok" | "submitting" | "queued" | "running" | "succeeded" | "failed" | "canceled" | "unknown";
export type BlueprintWaitingReason = "estimate_missing" | "estimate_stale" | "over_run_ceiling" | "over_step_allowance";

export interface BlueprintStepWaiting {
  reason: BlueprintWaitingReason;
  estimate: number | null;
  remaining: number;
  unit: string;
}

export interface BlueprintOperationApproval {
  scope: BlueprintRunScope;
  operation: string;
  actor: string;
  unit: string;
  maximum: number;
  runCeilingException: boolean;
}

export interface BlueprintJobUpdate {
  job: string;
  state: Extract<BlueprintOperationState, "queued" | "running" | "succeeded" | "failed" | "canceled" | "unknown">;
  outcome: string | null;
  outputs: Record<string, BlueprintOutputValue[]>;
}

export interface BlueprintSteps {
  blueprintEstimate(request: BlueprintStepRequest): BlueprintStepEstimate | null;
  blueprintSubmit(request: BlueprintStepRequest): BlueprintJobUpdate;
  blueprintReconcile(request: BlueprintStepRequest): BlueprintJobUpdate;
  blueprintCancel(request: BlueprintStepRequest): BlueprintJobUpdate;
}
