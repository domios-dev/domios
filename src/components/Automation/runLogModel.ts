import { decisionKind } from "./nodeDecision";
import { projectAutomationGraph, type PresentationEdge } from "./projection";
import { automationKey, type AutomationGraph, type AutomationTransition, type AutomationNode } from "./types";

import type { GraphRun, RunEvent } from "../../lib/blueprintRun";
export type { GraphRun, RunEvent } from "../../lib/blueprintRun";

const routeKey = (source: string, bucket: string, handler: string | null) => JSON.stringify([source, bucket, handler]);

function transitionKey(node: AutomationNode, transition: AutomationTransition) {
  return JSON.stringify([automationKey(node.identity), transition.id]);
}

export function graphRuns(graph: AutomationGraph): GraphRun[] {
  const presentation = projectAutomationGraph(graph);
  const names = new Map(presentation.nodes.map((node) => [node.key, node.participant?.name ?? node.action?.handler?.label ?? node.action?.handler?.id ?? node.action?.bucket ?? "Action"]));
  const pieces = graph.nodes.flatMap((node) => [...(node.history ?? [])].reverse().map((transition) => ({ node, transition, key: transitionKey(node, transition) })));
  const byTransition = new Map(pieces.map((piece) => [piece.key, piece]));
  const roots = new Map(graph.nodes.filter((node) => node.identity.kind === "sentinel").map((node) => [node.identity.id, node]));
  const routes = new Map<string, PresentationEdge>();
  for (const edge of presentation.edges) {
    const key = routeKey(edge.from, edge.bucket, edge.handler_id);
    if (!routes.has(key)) routes.set(key, edge);
  }
  const eventsFor = ({ node, transition }: typeof pieces[number]): RunEvent[] => {
    const source = automationKey(node.identity);
    const events: RunEvent[] = transition.handlers.map((handler) => {
      const edge = transition.routes_current === false ? undefined : routes.get(routeKey(source, transition.bucket, handler.handler_id));
      const command = handler.command_result;
      const duration = command && Number.isSafeInteger(command.started_at) && Number.isSafeInteger(command.finished_at) && command.started_at >= 0 && command.finished_at >= command.started_at ? command.finished_at - command.started_at : null;
      return {
        classifier: decisionKind(transition), confidence: transition.confidence, reason: transition.reason,
        at: handler.accepted_at ?? transition.accepted_at, node: node.name, outcome: transition.bucket,
        target: edge ? names.get(edge.to) ?? null : handler.handler_id,
        status: handler.status ?? "Unknown", duration,
        edgeId: handler.status === "skipped" || handler.status === "noop" ? null : edge?.id ?? null,
      };
    });
    if (!events.length) events.push({ classifier: decisionKind(transition), confidence: transition.confidence, reason: transition.reason, at: transition.accepted_at, node: node.name, outcome: transition.bucket, target: null, status: "No action", duration: null, edgeId: null });
    return events;
  };
  const runs: GraphRun[] = (graph.runs ?? []).flatMap((run) => {
    const root = roots.get(run.root_node);
    if (!root) return [];
    const entries = run.transitions.flatMap((entry) => {
      const piece = byTransition.get(JSON.stringify([automationKey(entry.node), entry.transition_id]));
      return piece ? [{ ...piece, parent: entry.parent }] : [];
    });
    const children = new Map<string, typeof entries>();
    for (const entry of entries) {
      if (!entry.parent) continue;
      const key = routeKey(entry.parent.node, entry.parent.transition, entry.parent.handler);
      const siblings = children.get(key) ?? [];
      siblings.push(entry);
      children.set(key, siblings);
    }
    const visited = new Set<string>();
    const events: RunEvent[] = [];
    const visit = (entry: typeof entries[number]) => {
      if (visited.has(entry.key)) return;
      visited.add(entry.key);
      eventsFor(entry).forEach((event, index) => {
        events.push(event);
        const handler = entry.transition.handlers[index];
        if (handler) children.get(routeKey(entry.node.identity.id, entry.transition.id, handler.handler_id))?.forEach(visit);
      });
    };
    entries.filter((entry) => entry.parent === null).forEach(visit);
    entries.forEach(visit);
    return [{ key: run.id, at: run.started_at, name: root.name, events, state: run.state, conversation: root.conversation, incomplete: run.incomplete || entries.length < run.transitions.length }];
  });
  for (const piece of pieces) {
    if (piece.transition.causal_run) continue;
    runs.push({ key: piece.key, at: piece.transition.accepted_at, name: piece.node.name, events: eventsFor(piece), state: "unlinked", conversation: piece.node.conversation, incomplete: false });
  }
  return runs.sort((a, b) => b.at - a.at || a.key.localeCompare(b.key));
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
  return `${time} ${event.node} → ${event.outcome}${event.target ? ` → ${event.target}` : ""} (${event.status}${event.confidence != null ? ` · confidence ${Math.round(event.confidence * 100)}%` : ""}${event.reason ? ` · ${event.reason}` : ""}${event.duration === null ? "" : `, ${event.duration} ms`})`;
}
