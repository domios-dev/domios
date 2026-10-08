import { projectAutomationGraph, type PresentationEdge } from "./projection";
import { automationKey, type AutomationGraph, type AutomationTransition } from "./types";

export interface RunEvent {
  at: number;
  node: string;
  outcome: string;
  target: string | null;
  status: string;
  duration: number | null;
  edgeId: string | null;
}

export interface GraphRun {
  key: string;
  at: number;
  name: string;
  events: RunEvent[];
}

function eventEdge(edges: readonly PresentationEdge[], source: string, transition: AutomationTransition, handlerId: string) {
  return edges.find((edge) => edge.from === source && edge.bucket === transition.bucket && edge.handler_id === handlerId);
}

export function graphRuns(graph: AutomationGraph): GraphRun[] {
  const presentation = projectAutomationGraph(graph);
  const names = new Map(presentation.nodes.map((node) => [node.key, node.participant?.name ?? node.action?.handler?.label ?? node.action?.handler?.id ?? node.action?.bucket ?? "Action"]));
  return graph.nodes.flatMap((node) => (node.history ?? []).map((transition) => {
    const source = automationKey(node.identity);
    const events: RunEvent[] = transition.handlers.map((handler) => {
      const edge = transition.routes_current === false ? undefined : eventEdge(presentation.edges, source, transition, handler.handler_id);
      const command = handler.command_result;
      const duration = command && Number.isSafeInteger(command.started_at) && Number.isSafeInteger(command.finished_at) && command.started_at >= 0 && command.finished_at >= command.started_at ? command.finished_at - command.started_at : null;
      return {
        at: handler.accepted_at ?? transition.accepted_at, node: node.name, outcome: transition.bucket,
        target: edge ? names.get(edge.to) ?? null : handler.handler_id,
        status: handler.status ?? "Unknown", duration,
        edgeId: handler.status === "skipped" ? null : edge?.id ?? null,
      };
    });
    events.sort((a, b) => a.at - b.at);
    if (!events.length) events.push({ at: transition.accepted_at, node: node.name, outcome: transition.bucket, target: null, status: "No action", duration: null, edgeId: null });
    return { key: JSON.stringify([source, transition.id]), at: transition.accepted_at, name: node.name, events };
  })).sort((a, b) => b.at - a.at || a.key.localeCompare(b.key));
}

export function replayEdges(run: GraphRun): string[] {
  return run.events.flatMap((event) => event.edgeId === null ? [] : [event.edgeId]);
}

export function replayFrame(edges: readonly string[], elapsedMs: number, reducedMotion: boolean) {
  const index = Math.floor(Math.max(0, elapsedMs) / 850);
  return { highlighted: edges, active: reducedMotion || index >= edges.length ? null : edges[index], progress: reducedMotion ? 0 : Math.max(0, elapsedMs) % 850 / 850 };
}

export function runEventLine(event: RunEvent): string {
  const time = new Date(event.at).toLocaleTimeString();
  return `${time} ${event.node} → ${event.outcome}${event.target ? ` → ${event.target}` : ""} (${event.status}${event.duration === null ? "" : `, ${event.duration} ms`})`;
}
