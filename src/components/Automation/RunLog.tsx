import { useState } from "react";
import { NodeKindBadge } from "./NodeKindBadge";
import { graphRuns, runEventLine, type GraphRun } from "./runLogModel";
import type { AutomationGraph } from "./types";
import { AUTOMATION_TOOL } from "./styles";

export function RunLog({ graph, selected, onReplay }: { graph: AutomationGraph; selected: string | null; onReplay: (run: GraphRun) => void }) {
  const [height, setHeight] = useState(288);
  const runs = graphRuns(graph);
  return <section id="blueprint-run-log" aria-label="Blueprint runs" style={{ height, minHeight: 160, maxHeight: "65vh", resize: "vertical" }} className="shrink-0 overflow-auto border-t border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] p-3 text-xs">
    <label className="mb-2 flex items-center gap-2 text-[var(--ct-muted-fg)]">Log height<input aria-label="Run log height" type="range" min="160" max="640" step="16" value={height} onChange={(event) => setHeight(Number(event.target.value))} /></label>
    <p className="mb-2 text-[var(--ct-muted-fg)]">Accepted runs · newest first</p>
    {graph.nodes.some((node) => node.history_truncated) && <p role="status">Some older runs are omitted from this bounded history.</p>}
    {!runs.length && <p role="status">No accepted runs recorded.</p>}
    <ol className="space-y-3">{runs.map((run) => <li key={run.key}>
      <button type="button" className={AUTOMATION_TOOL} aria-pressed={run.key === selected} onClick={() => onReplay(run)} aria-label={`Replay ${run.name} at ${new Date(run.at).toLocaleString()}`}>{new Date(run.at).toLocaleString()} · {run.name} · {run.state === "unlinked" ? "Recorded event" : run.state}</button>
      {run.incomplete && <p role="status">Part of this run is outside the retained history.</p>}
      {run.conversation?.enabled && <p>Conversation: {run.conversation.exchanges}/{run.conversation.safety_limit} exchanges · {run.conversation.actions}/{run.conversation.safety_limit} actions · limit {run.conversation.rate_limit_per_minute}/minute</p>}
      <ol className="mt-1 space-y-1 pl-2">{run.events.map((event, index) => <li key={index} className="break-words text-[var(--ct-muted-fg)]">{event.classifier && <NodeKindBadge kind={event.classifier} />} {runEventLine(event)}</li>)}</ol>
    </li>)}</ol>
  </section>;
}
