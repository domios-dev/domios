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

export type BlueprintPortKind = import("./gen/PortKind").PortKind;

export type BlueprintStepPort = Omit<import("./gen/StepPort").StepPort, "schema"> & {schema?: BlueprintParameterSchema};

export type BlueprintStepDescriptor = Omit<import("./gen/StepDescriptor").StepDescriptor, "contractVersion" | "params" | "inputs" | "outputs"> & {contractVersion: 1; params: BlueprintParameterSchema; inputs: Record<string, BlueprintStepPort>; outputs: Record<string, BlueprintStepPort>};

export type BlueprintRunScope = import("./gen/RunScope").RunScope;

export type BlueprintInputLineage = import("./gen/InputLineage").InputLineage;

export type BlueprintStepContext = import("./gen/StepContext").StepContext;

export type BlueprintPortValue = Exclude<import("./gen/PortValue").PortValue, {kind: "media_source" | "media"}> | MediaPortValue<"media">;

export type BlueprintOutputValue = Exclude<import("./gen/PortValue").PortValue, {kind: "media" | "media_source"}> | MediaPortValue<"media_source">;

export type BlueprintPortValues = Record<string, BlueprintPortValue[]>;

export type BlueprintStepRequest = Omit<import("./gen/StepRequest").StepRequest, "params" | "inputs"> & {params: Record<string, unknown>; inputs: BlueprintPortValues; job: string | null; deadlineMs: number};

export type BlueprintOperationState = import("./gen/OperationState").OperationState;
export type BlueprintJobUpdate = Omit<import("./gen/JobUpdate").JobUpdate, "state" | "outputs"> & {state: Extract<BlueprintOperationState, "queued" | "running" | "succeeded" | "failed" | "canceled" | "unknown">; outputs: Record<string, BlueprintOutputValue[]>};

export interface BlueprintSteps {
  blueprintSubmit(request: BlueprintStepRequest): BlueprintJobUpdate;
  blueprintReconcile(request: BlueprintStepRequest): BlueprintJobUpdate;
  blueprintCancel(request: BlueprintStepRequest): BlueprintJobUpdate;
}

type MediaPortValue<Kind extends "media" | "media_source"> = Omit<Extract<import("./gen/PortValue").PortValue, {kind: Kind}>, "media"> & {media: Exclude<BlueprintPortKind, "text" | "json">};
