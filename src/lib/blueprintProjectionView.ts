import type { BlueprintDocument, BlueprintExpandedCall, BlueprintJournalNode, BlueprintProjection } from "./blueprintApi";
import type { BlueprintRetainedProjection } from "../../gen/BlueprintRetainedProjection";
import type { ExpandedCall } from "../../gen/ExpandedCall";

type NestedRun = NonNullable<BlueprintRetainedProjection["nested_run"]>;
export type ExpandedCallInput = Omit<ExpandedCall, "graph" | "children"> & { graph: unknown; children: readonly ExpandedCallInput[] };
export type BlueprintProjectionInput = Omit<BlueprintRetainedProjection, "call_graph" | "expanded_calls" | "nested_run"> & {
  call_graph?: unknown;
  expanded_calls?: readonly ExpandedCallInput[];
  nested_run?: Omit<NestedRun, "nodes" | "definition_version"> & Partial<Pick<NestedRun, "definition_version">> & { nodes: readonly unknown[] } | null;
};

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function isBlueprintDocument(value: unknown): value is BlueprintDocument {
  return record(value) && value.schema_version === 2 && (value.kind === "pipeline" || value.kind === "watch")
    && typeof value.name === "string" && Array.isArray(value.steps)
    && value.steps.every((step: unknown) => record(step) && typeof step.id === "string")
    && (value.other === undefined || (Array.isArray(value.other) && value.other.every((step: unknown) => record(step) && typeof step.id === "string")));
}

function expandedView(call: ExpandedCallInput): BlueprintExpandedCall {
  return { ...call, graph: isBlueprintDocument(call.graph) ? call.graph : null, children: call.children.map(expandedView) };
}

function journalNode(value: unknown): value is BlueprintJournalNode {
  return record(value) && typeof value.step_name === "string" && typeof value.state === "string"
    && (value.call_stack === undefined || (Array.isArray(value.call_stack) && value.call_stack.every((frame: unknown) => record(frame) && typeof frame.node === "string" && typeof frame.id === "string" && typeof frame.hash === "string")));
}

export function blueprintProjectionView(value: BlueprintProjectionInput): BlueprintProjection {
  return {
    historical_graph_available: value.historical_graph_available,
    graph_definition_version: value.graph_definition_version,
    plugin_nodes: value.plugin_nodes,
    call_graph: isBlueprintDocument(value.call_graph) ? value.call_graph : null,
    expanded_calls: value.expanded_calls?.map(expandedView),
    nested_run: value.nested_run ? { ...value.nested_run, nodes: value.nested_run.nodes.flatMap((node) => journalNode(node) ? [node] : []) } : undefined,
  };
}
