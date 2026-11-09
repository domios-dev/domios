export const BLUEPRINT_API_BASE = "/api/automation/blueprints";

export type BlueprintValue = null | boolean | number | string | readonly BlueprintValue[] | { readonly [key: string]: BlueprintValue };
export type BlueprintDataBindings = Readonly<Record<string, string>>;
export interface BlueprintPort {
  type?: string;
  $ref?: string;
  properties?: Readonly<Record<string, BlueprintPort>>;
  items?: BlueprintPort;
  required?: readonly string[];
  description?: string;
}
export interface BlueprintIrNode {
  id: string;
  name?: string;
  does?: string;
  uses?: string;
  run?: BlueprintValue;
  code?: { lang: string; src?: string; file?: string; caps?: readonly string[]; limits?: BlueprintValue };
  call?: { ref: string; with?: Readonly<Record<string, BlueprintValue>>; pin?: BlueprintCallPin };
  in?: BlueprintPort;
  out?: BlueprintPort;
  bind?: BlueprintDataBindings;
  routes?: Readonly<Record<string, string>>;
  buckets?: readonly string[];
  outcomes?: readonly string[];
  caps?: readonly string[];
  [key: string]: unknown;
}
export interface BlueprintDocument {
  schema_version: 2;
  kind: "pipeline" | "watch";
  name: string;
  intent?: string;
  note?: string;
  trigger?: BlueprintValue;
  inputs?: Readonly<Record<string, BlueprintPort>>;
  types?: Readonly<Record<string, BlueprintPort>>;
  steps: readonly BlueprintIrNode[];
  other?: readonly BlueprintIrNode[];
  result?: { from?: string; to?: BlueprintValue; type?: BlueprintPort };
  imports?: BlueprintValue;
  [key: string]: unknown;
}
export type BlueprintCallPin = import("../../gen/CallPin").CallPin;
export type BlueprintExpandedCall = Omit<import("../../gen/ExpandedCall").ExpandedCall, "graph" | "children"> & {
  graph: BlueprintDocument | null;
  children: readonly BlueprintExpandedCall[];
};

export type BlueprintCallFrame = import("../../gen/CallFrame").CallFrame;
type WireJournal = import("../../gen/BlueprintJournalPreview").BlueprintJournalPreview;
export type BlueprintJournalNode = Pick<WireJournal, "entry_id" | "run_id" | "step_index" | "reason" | "outcome" | "result_summary" | "started_at_ms" | "ended_at_ms"> & Required<Pick<WireJournal, "step_name" | "state">> & {
  definition_version?: WireJournal["definition_version"] | null;
  parent_step_index?: WireJournal["parent_step_index"] | null;
  call_stack?: readonly BlueprintCallFrame[];
  [key: string]: unknown;
};
export type BlueprintExpandedGraph = import("../../gen/ExpandedGraph").ExpandedGraph;
export type BlueprintPluginPortKind = import("../../gen/PortKind").PortKind;
export type BlueprintPluginPort = import("../../gen/StepPort").StepPort;
export type BlueprintPluginNode = Pick<import("../../gen/AvailablePluginNode").AvailablePluginNode, "available"> & Partial<Omit<import("../../gen/AvailablePluginNode").AvailablePluginNode, "available">>;
export type BlueprintProjection = Omit<import("../../gen/BlueprintRetainedProjection").BlueprintRetainedProjection, "call_graph" | "expanded_calls" | "nested_run" | "plugin_nodes"> & {
  call_graph?: BlueprintDocument | null;
  expanded_calls?: readonly BlueprintExpandedCall[];
  nested_run?: Omit<NonNullable<import("../../gen/BlueprintRetainedProjection").BlueprintRetainedProjection["nested_run"]>, "nodes" | "definition_version"> & Partial<Pick<NonNullable<import("../../gen/BlueprintRetainedProjection").BlueprintRetainedProjection["nested_run"]>, "definition_version">> & { nodes: readonly BlueprintJournalNode[] } | null;
  plugin_nodes?: Readonly<Record<string, BlueprintPluginNode>>;
};

export interface BlueprintSemanticDiff {
  added: readonly string[];
  removed: readonly string[];
  changed: Readonly<Record<string, readonly string[]>>;
  document_fields: readonly string[];
}
export interface BlueprintClosureDiff {
  root: BlueprintSemanticDiff;
  callees: Readonly<Record<string, { before: BlueprintCallPin | null; after: BlueprintCallPin | null; fields: BlueprintSemanticDiff | null }>>;
}
export interface BlueprintViewerHistory {
  current_version: number;
  history_truncated: boolean;
  plugin_nodes?: Readonly<Record<string, BlueprintPluginNode>>;
  versions: readonly { version: number; replaces: number | null; diff: BlueprintClosureDiff | null; before?: BlueprintExpandedGraph | null; after?: BlueprintExpandedGraph }[];
}
export interface BlueprintProjectEntry { path: string; name: string | null; intent: string | null; error: string | null }
export interface BlueprintProjectView { path: string; document: BlueprintDocument }
