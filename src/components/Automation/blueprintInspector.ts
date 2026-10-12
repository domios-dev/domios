import { bucketDefinitions } from "./nodeDecision";
import { probeStateLabel } from "./probeProgress";
import type { BlueprintNode } from "./blueprintModel";
import type { AutomationGraph, AutomationHandler, AutomationHandlerResult, AutomationIdentity } from "./types";

import { actionKind, blueprintNodeKinds, blueprintTriggers } from "./blueprintKinds";

export interface BlueprintInspection {
  name: string;
  type: string;
  purpose: string;
  properties: [string, string][];
  prompt: string | null;
  routes: { name: string; description: string }[];
  code: { text: string; language: string; truncated: boolean } | null;
  command: { handler: AutomationHandler; result: AutomationHandlerResult | null } | null;
  files: { identity: AutomationIdentity; bucket: string | null; handler: string | null } | null;
}

function cadence(ms: number | null) {
  return ms ? `${ms / 1000} s` : null;
}

export function inspectBlueprintNode(node: BlueprintNode, graph: AutomationGraph): BlueprintInspection {
  const stage = node.stage;
  const definition = stage.detail?.definition;
  const properties: [string, string][] = [];
  const add = (name: string, value: string | null | undefined) => { if (value) properties.push([name, value]); };
  const base = { name: node.title, type: blueprintNodeKinds[node.type].label, prompt: null, routes: [], code: null, command: null, files: null };
  if (node.type === "trigger") {
    add("Stage", stage.name);
    add("When", node.subtitle === "Schedule" ? node.title : `${node.title} · ${node.subtitle}`);
    add("Cadence", cadence(stage.cadence_ms));
    add("Schedule", stage.schedule);
    add("Expires", stage.expires_at ? new Date(stage.expires_at).toLocaleString() : null);
    add("State", probeStateLabel(stage));
    add("Last probe", stage.detail?.last_probe_at != null ? new Date(stage.detail.last_probe_at).toLocaleString() : null);
    return { ...base, purpose: blueprintTriggers[node.trigger ?? "schedule"].purpose, properties };
  }
  if (node.type === "source") {
    const source = stage.source;
    add("Kind", node.subtitle);
    add("Pattern", source?.pattern ? `/${source.pattern}/` : null);
    add("Settles after", source?.idle_ms ? `${source.idle_ms} ms` : null);
    add("Input", source?.type === "input" ? `${source.kind ?? "text"} ${source.ref_type ?? ""}`.trim() : null);
    add("Converts to", source?.convert_to ?? null);
    add("State", probeStateLabel(stage));
    add("Last probe", stage.detail?.last_probe_at != null ? new Date(stage.detail.last_probe_at).toLocaleString() : null);
    add("Output", node.outputs.find((pin) => pin.kind === "data")?.label);
    const probe = definition?.probe;
    return {
      ...base, properties,
      purpose: probe ? "Runs this command and hands its output to the decision." : "Collects what happened and hands it to the decision.",
      code: probe ? { text: probe, language: definition?.command_language ?? "", truncated: !!definition?.probe_truncated } : null,
      files: probe ? { identity: stage.identity, bucket: null, handler: null } : null,
    };
  }
  if (node.type === "agent" || node.type === "logic") {
    const fallback = graph.configured_fallback;
    if (node.type === "agent") {
      add("Agent", node.title.split(" · ")[0]);
      add("Model", stage.executor?.model ?? fallback?.model ?? null);
      add("Adapter", stage.executor?.adapter_id ?? fallback?.adapter_id ?? null);
    }
    add("Receives", node.inputs.find((pin) => pin.kind === "data")?.label.replace(/^receives /, ""));
    return {
      ...base, properties, routes: bucketDefinitions(stage),
      purpose: node.type === "agent" ? "Reads what the source received and chooses one route by its instruction." : "Matches what the source received against fixed rules and chooses one route.",
      prompt: node.type === "agent" ? definition?.question ?? null : null,
    };
  }
  const action = node.action!;
  const handler = action.handler;
  if (!handler) return { ...base, purpose: `${action.overflow} more actions are declared on this stage; open its decision to see every route.`, properties };
  add("Route", action.bucket);
  add("Action", handler.type === "blueprint" ? "blueprint" : handler.type);
  add("Target", action.target?.name ?? (handler.target ? "Unavailable target" : null));
  add("Timeout", handler.timeout_ms ? `${handler.timeout_ms} ms` : null);
  add("Last result", action.result?.status ?? "Not run yet");
  const command = actionKind(handler.type)?.code ? definition?.commands?.find((entry) => entry.bucket === action.bucket && entry.handler_id === handler.id) : undefined;
  return {
    ...base, properties,
    purpose: `Runs when the decision chooses ${action.bucket}; it receives the source output as its payload.`,
    code: typeof command?.command === "string" ? { text: command.command, language: definition?.command_language ?? "", truncated: command.command_truncated } : null,
    command: actionKind(handler.type)?.code ? { handler, result: action.result } : null,
    files: command ? { identity: stage.identity, bucket: action.bucket, handler: handler.id } : null,
  };
}
