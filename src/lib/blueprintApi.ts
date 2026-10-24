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
export interface BlueprintCallPin { id: string; hash: string }
export interface BlueprintExpandedCall {
  node: string;
  pin: BlueprintCallPin;
  graph: BlueprintDocument;
  children: readonly BlueprintExpandedCall[];
}
export interface BlueprintCallFrame extends BlueprintCallPin { node: string }
export interface BlueprintJournalNode {
  entry_id?: number;
  run_id?: string;
  step_index?: number;
  definition_version?: number | null;
  parent_step_index?: number | null;
  step_name: string;
  state: string;
  reason?: string | null;
  outcome?: string | null;
  result_summary?: string | null;
  started_at_ms?: number;
  ended_at_ms?: number | null;
  call_stack?: readonly BlueprintCallFrame[];
  [key: string]: unknown;
}
export interface BlueprintProjection {
  historical_graph_available?: boolean;
  graph_definition_version?: number | null;
  call_graph?: BlueprintDocument | null;
  expanded_calls?: readonly BlueprintExpandedCall[];
  nested_run?: { id: string | null; definition_version?: number | null; nodes: readonly BlueprintJournalNode[] } | null;
  projection_error?: string | null;
}
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
  versions: readonly { version: number; replaces: number | null; diff: BlueprintClosureDiff | null; before?: { root: BlueprintDocument; calls: readonly BlueprintExpandedCall[] } | null; after?: { root: BlueprintDocument; calls: readonly BlueprintExpandedCall[] } }[];
}
export interface BlueprintProjectEntry { path: string; name: string | null; intent: string | null; error: string | null }
export interface BlueprintProjectView { path: string; document: BlueprintDocument }
