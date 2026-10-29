type SchemaNotes = { description?: string; default?: unknown };
export type BlueprintParameterSchema = SchemaNotes & (
  | { type: "object" | ["object", "null"]; properties: Record<string, BlueprintParameterSchema>; required?: string[]; additionalProperties?: false }
  | { type: "array" | ["array", "null"]; items: BlueprintParameterSchema; minItems?: number; maxItems: number }
  | { type: "string" | ["string", "null"]; maxLength?: number; enum?: (string | null)[]; const?: string | null }
  | { type: "number" | "integer" | ["number", "null"] | ["integer", "null"]; minimum?: number; maximum?: number; enum?: (number | null)[]; const?: number | null }
  | { type: "boolean" | ["boolean", "null"]; enum?: (boolean | null)[]; const?: boolean | null }
  | { type: "null" }
  | { oneOf: BlueprintParameterSchema[] }
);

export type BlueprintPortKind = "text" | "json" | "image" | "audio" | "video" | "file";

export interface BlueprintStepPort {
  kind: BlueprintPortKind;
  minItems: number;
  maxItems: number;
  schema?: BlueprintParameterSchema;
}

export interface BlueprintStepDescriptor {
  contractVersion: 1;
  revision: number;
  id: string;
  title: string;
  description: string;
  params: BlueprintParameterSchema;
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
export type BlueprintWaitingReason = "estimate_missing" | "estimate_stale" | "over_run_ceiling" | "over_step_allowance" | "owner_confirmation";

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
