import { useId, useState } from "react";
import { Bell, Play, Workflow } from "lucide-react";
import { AUTOMATION_TOOL } from "./styles";
import type { PipelineView, PipelineViewNode } from "./pipelineModel";
import { layoutPipelineChain, PIPELINE_CANVAS_HEIGHT, PIPELINE_CARD_HEIGHT, PIPELINE_CARD_WIDTH } from "./pipelineLayout";

function stateTone(state: string, current: boolean): string {
  if (current || state === "running" || state === "delivering") return "text-[var(--ct-accent)]";
  if (state === "failed") return "text-[var(--ct-red)]";
  if (state === "done" || state === "delivered") return "text-[var(--ct-green)]";
  return "text-[var(--ct-muted-fg)]";
}

function stateSurface(state: string, current: boolean): string {
  if (current) return "border-[color-mix(in_srgb,var(--ct-accent)_45%,var(--ct-border))] shadow-[inset_0_0_16px_color-mix(in_srgb,var(--ct-accent)_7%,transparent),var(--ct-shadow-sm)]";
  if (state === "failed") return "border-[color-mix(in_srgb,var(--ct-red)_60%,var(--ct-border))] shadow-[var(--ct-shadow-sm)]";
  return "border-[var(--ct-border)] shadow-[var(--ct-shadow-sm)]";
}

function eventTime(value: number | null): string {
  return value === null ? "No event recorded" : new Date(value).toLocaleString();
}

function PipelineNodeCard({ node }: { node: PipelineViewNode }) {
  const Icon = node.kind === "trigger" ? Play : node.kind === "result" ? Bell : Workflow;
  return <article
    aria-current={node.kind === "step" && node.current ? "step" : undefined}
    className={`flex h-full flex-col gap-1 rounded-lg border bg-[var(--ct-titlebar-bg)] px-3 py-2.5 ${stateSurface(node.state, node.current)}`}
  >
    <div className="flex items-center gap-1.5 text-[0.6875rem] uppercase tracking-wider text-[var(--ct-muted-fg)]">
      <Icon size={12} aria-hidden="true" />
      <span>{node.kind === "step" ? "Step" : node.kind}</span>
      <span className={`ml-auto flex items-center gap-1 normal-case tracking-normal ${stateTone(node.state, node.current)}`}>
        <span className="h-1.5 w-1.5 rounded-full bg-current" aria-hidden="true" />{node.state}
      </span>
    </div>
    <h4 className="truncate text-[length:var(--ct-font-size-sm)] font-semibold text-[var(--ct-fg)]" title={node.title}>{node.title}</h4>
    {node.kind === "result"
      ? <>
        <p className="truncate text-xs text-[var(--ct-muted-fg)]" title={node.targetSummary ?? "Target unavailable"}>{node.targetSummary ?? "Target unavailable"}</p>
        {node.summary && <p className="truncate text-xs text-[var(--ct-muted-fg)]">{node.summary}</p>}
      </>
      : <>
        <p className="truncate text-xs text-[var(--ct-muted-fg)]" title={node.summary}>{node.summary || "No summary"}</p>
        {node.kind === "step" && <>
          <time className="truncate text-[0.6875rem] text-[var(--ct-muted-fg)]" dateTime={node.lastEventAtMs === null ? undefined : new Date(node.lastEventAtMs).toISOString()}>
            Last event · {eventTime(node.lastEventAtMs)}
          </time>
          <p className="line-clamp-2 text-[0.6875rem] text-[var(--ct-muted-fg)]">{node.resultSummary ?? "No result summary"}</p>
        </>}
      </>}
  </article>;
}

export function BlueprintPipelineCanvas({ pipeline, initialRunId }: { pipeline: PipelineView; initialRunId?: string }) {
  const [showRuns, setShowRuns] = useState(Boolean(initialRunId));
  const [runId, setRunId] = useState<string | null>(initialRunId ?? null);
  const selected = pipeline.runs?.find((run) => run.id === runId)?.pipeline ?? pipeline;
  return <div className="space-y-3">
    <button type="button" className={AUTOMATION_TOOL} aria-expanded={showRuns} onClick={() => setShowRuns(!showRuns)}>Runs ({pipeline.runs?.length ?? 0})</button>
    {showRuns && <div role="group" aria-label="Pipeline runs" className="flex flex-wrap gap-2">
      <button type="button" className={AUTOMATION_TOOL} aria-pressed={runId === null} onClick={() => setRunId(null)}>Latest</button>
      {pipeline.runs?.map((run) => <button type="button" className={`${AUTOMATION_TOOL} aria-pressed:border-[var(--ct-accent)]`} key={run.id} aria-label={`Run ${run.id}`} aria-pressed={runId === run.id} onClick={() => setRunId(run.id)}>{eventTime(run.at)} · {run.state}</button>)}
    </div>}
    <PipelineChain pipeline={selected} />
  </div>;
}

function PipelineChain({ pipeline }: { pipeline: PipelineView }) {
  const markerId = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const layout = layoutPipelineChain(pipeline.nodes);
  const byKey = new Map(pipeline.nodes.map((node) => [node.key, node]));
  return <div role="group" aria-label={`${pipeline.name} pipeline`} className="min-w-0 overflow-x-auto rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]">
    <div className="relative" style={{ width: layout.width, height: layout.height, minHeight: PIPELINE_CANVAS_HEIGHT }}>
      <svg className="pointer-events-none absolute inset-0" width={layout.width} height={layout.height} aria-hidden="true">
        <defs><marker id={`pipeline-arrow-${markerId}`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M 0 0 L 10 5 L 0 10 z" fill="var(--ct-muted-fg)" /></marker></defs>
        {layout.edges.map((edge) => <path key={`${edge.from}:${edge.to}`} d={`M ${edge.x + 8} ${edge.y} L ${edge.x + edge.width - 8} ${edge.y}`} stroke="var(--ct-muted-fg)" strokeWidth="1.5" markerEnd={`url(#pipeline-arrow-${markerId})`} />)}
      </svg>
      {layout.edges.map((edge) => edge.label ? <span key={`label:${edge.from}:${edge.to}`} title={edge.label} className="absolute z-10 max-w-[4.5rem] -translate-x-1/2 truncate rounded-full border border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-1.5 py-0.5 text-[0.625rem] text-[var(--ct-muted-fg)]" style={{ left: edge.x + edge.width / 2, top: edge.y - 28 }}>{edge.label}</span> : null)}
      {layout.nodes.map((box) => {
        const node = byKey.get(box.key);
        if (!node) return null;
        return <div key={box.key} className="absolute" style={{ left: box.x, top: box.y, width: Math.min(box.width, PIPELINE_CARD_WIDTH), height: Math.min(box.height, PIPELINE_CARD_HEIGHT) }}>
          <PipelineNodeCard node={node} />
        </div>;
      })}
    </div>
  </div>;
}
