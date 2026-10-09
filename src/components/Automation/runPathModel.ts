import type { GraphRun } from "../../lib/blueprintRun";

export function runPathSteps(run: GraphRun) {
  return run.events.flatMap((event, index) => {
    const traversed = event.edgeId !== null && event.target !== null;
    const source = { name: event.node, outcome: event.outcome, at: event.at, duration: null as number | null, toNext: traversed, wire: event.edgeId };
    return traversed && event.target && event.target !== run.events[index + 1]?.node
      ? [source, { name: event.target, outcome: event.status, at: event.at, duration: event.duration, toNext: false, wire: null }]
      : [source];
  });
}
