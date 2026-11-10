export type BlueprintParameterSchema = import("./gen/AuthoredSchema").AuthoredSchema;

export type BlueprintPortKind = import("./gen/PortKind").PortKind;

export type BlueprintStepPort = import("./gen/StepPort").StepPort;

export type BlueprintStepDescriptor = import("./gen/StepDescriptor").StepDescriptor;

export type BlueprintRunScope = import("./gen/RunScope").RunScope;

export type BlueprintInputLineage = import("./gen/InputLineage").InputLineage;

export type BlueprintStepContext = import("./gen/StepContext").StepContext;

export type BlueprintPortValue = Exclude<import("./gen/PortValue").PortValue, {kind: "media_source" | "media"}> | MediaPortValue<"media">;

export type BlueprintOutputValue = Exclude<import("./gen/PortValue").PortValue, {kind: "media" | "media_source"}> | MediaPortValue<"media_source">;

export type BlueprintPortValues = Record<string, BlueprintPortValue[]>;

export type BlueprintStepRequest = Omit<import("./gen/BlueprintStepRequest").BlueprintStepRequest, "params" | "inputs"> & {params: Record<string, unknown>; inputs: BlueprintPortValues};

export type BlueprintOperationState = import("./gen/OperationState").OperationState;
export type BlueprintJobUpdate = Omit<import("./gen/JobUpdate").JobUpdate, "state" | "outputs"> & {state: Extract<BlueprintOperationState, "queued" | "running" | "succeeded" | "failed" | "canceled" | "unknown">; outputs: Record<string, BlueprintOutputValue[]>};

export interface BlueprintSteps {
  blueprintSubmit(request: BlueprintStepRequest): BlueprintJobUpdate;
  blueprintReconcile(request: BlueprintStepRequest): BlueprintJobUpdate;
  blueprintCancel(request: BlueprintStepRequest): BlueprintJobUpdate;
}

type MediaPortValue<Kind extends "media" | "media_source"> = Omit<Extract<import("./gen/PortValue").PortValue, {kind: Kind}>, "media"> & {media: Exclude<BlueprintPortKind, "text" | "json">};
