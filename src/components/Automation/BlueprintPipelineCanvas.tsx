import { blueprintSteps } from "./blueprintKinds";
import { useId, useState, type CSSProperties } from "react";
import { CanvasToolbar } from "../Canvas/CanvasToolbar";
import { Bell, List, Play, Workflow } from "lucide-react";
import { AUTOMATION_LABEL, AUTOMATION_TONE, AUTOMATION_TOOL } from "./styles";
import type { PipelineView, PipelineViewNode } from "./pipelineModel";
import { PIPELINE_CANVAS_HEIGHT, PIPELINE_CARD_HEIGHT, PIPELINE_CARD_WIDTH } from "./pipelineLayout";
import type { BlueprintViewerHistory } from "../../lib/blueprintApi";
import { useBlueprintGraph } from "./useBlueprintGraph";
import { toggleBlueprintExpansion, type BlueprintExpansion } from "./blueprintExpansion";
import { expandedCall } from "./blueprintGraphModel";
import { BlueprintPipelineNodeDetails } from "./BlueprintPipelineNodeDetails";
import { BlueprintSemanticDiff } from "./BlueprintSemanticDiff";
import { BlueprintProjectionValue } from "./BlueprintProjectionValue";
import { blueprintPipelineOutcome, canStopPipelineRun } from "./blueprintPipelineOutcome";

function stateTone(state: string, current = false): string {
  if (current || state === "running" || state === "delivering") return "var(--ct-accent)";
  if (["failed", "failing", "offline", "down"].includes(state)) return "var(--ct-red)";
  if (["done", "delivered", "online", "up"].includes(state)) return "var(--ct-green)";
  return "var(--ct-muted)";
}

const KIND_LABEL: Record<PipelineViewNode["kind"], string> = { trigger: "Trigger", step: "Step", result: "Result" };

function stateSurface(state: string, current: boolean): string {
  if (current) return "shadow-[inset_0_0_16px_color-mix(in_srgb,var(--ct-accent)_7%,transparent)]";
  if (["failed", "failing", "offline", "down"].includes(state)) return "ct-card--error";
  return "";
}

function eventTime(value: number | null): string {
  return value === null ? "No event recorded" : new Date(value).toLocaleString();
}

function PipelineNodeCard({ node }: { node: PipelineViewNode }) {
  const source = node.capability && Object.prototype.hasOwnProperty.call(blueprintSteps, node.capability) ? blueprintSteps[node.capability as keyof typeof blueprintSteps] : null;
  const Icon = node.kind === "trigger" ? Play : node.kind === "result" ? Bell : source?.icon ?? Workflow;
  const state = node.sourceState ?? node.state;
  return <article
    aria-current={node.kind === "step" && node.current ? "step" : undefined}
    className={`ct-card flex h-full flex-col gap-1 px-3 py-2.5 ${stateSurface(state, node.current)}`}
    style={{ "--ct-card-tone": AUTOMATION_TONE.blueprint } as CSSProperties}
  >
    <div className={`flex items-center gap-1.5 ${AUTOMATION_LABEL}`}>
      <Icon size={12} className="text-[var(--ct-card-tone)]" aria-hidden="true" />
      <span>{node.capability === "github" ? "GitHub source" : node.capability === "http" ? "HTTP health" : KIND_LABEL[node.kind]}</span>
      <span className="ml-auto flex items-center gap-1.5 font-medium" style={{ "--ct-detail-tone": stateTone(state, node.current) } as CSSProperties}>
        <span className="ct-detail__dot" aria-hidden="true" />{state}
      </span>
    </div>
    {node.definition && <div className="flex gap-2 text-xs"><span aria-label="Input type">in: {node.definition.in?.type ?? node.definition.in?.$ref ?? "unspecified"}</span><span aria-label="Output type">out: {node.definition.out?.type ?? node.definition.out?.$ref ?? "unspecified"}</span></div>}
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

export function BlueprintPipelineCanvas({ pipeline, initialRunId, onStop, loadHistory }: {
  pipeline: PipelineView; initialRunId?: string; onStop?: () => Promise<unknown>; loadHistory?: () => Promise<BlueprintViewerHistory>;
}) {
  const [showRuns, setShowRuns] = useState(Boolean(initialRunId));
  const [runId, setRunId] = useState<string | null>(initialRunId ?? null);
  const [showDiff, setShowDiff] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const selected = runId === null ? pipeline : pipeline.runs?.find((run) => run.id === runId)?.pipeline;
  const stop = selected ? blueprintPipelineOutcome(selected) : null;
  const stopRun = async () => {
    if (!onStop || !selected || !canStopPipelineRun(pipeline, selected) || stopping) return;
    setStopping(true); setError(null);
    try { await onStop(); } catch (error: unknown) { setError(String(error)); } finally { setStopping(false); }
  };
  const row = (pressed: boolean) => `ct-row ct-run-row${pressed ? " ct-row--selected" : ""}`;
  return <div className="relative flex min-w-0 flex-col gap-3 pt-14">
    <CanvasToolbar label="Pipeline graph controls" extras={<button type="button" className={AUTOMATION_TOOL} aria-expanded={showRuns} onClick={() => setShowRuns(!showRuns)}><List size={13} aria-hidden="true" />Runs<span className="text-[var(--ct-muted-fg)]">{pipeline.runs?.length ?? 0}</span></button>} />
    {showRuns && <div role="group" aria-label="Pipeline runs" className="ct-rows">
      <button type="button" className={row(runId === null)} aria-label="Latest run" aria-pressed={runId === null} onClick={() => setRunId(null)}>
        <span className="ct-detail__dot" style={{ "--ct-detail-tone": stateTone(pipeline.state) } as CSSProperties} aria-hidden="true" />
        <span className="ct-run-row__main">Latest</span>
        <span className="ct-run-row__meta">{pipeline.state}</span>
      </button>
      {pipeline.runs?.map((run) => <button type="button" className={row(runId === run.id)} key={run.id} aria-label={`Run ${run.id}`} aria-pressed={runId === run.id} onClick={() => setRunId(run.id)}>
        <span className="ct-detail__dot" style={{ "--ct-detail-tone": stateTone(run.state) } as CSSProperties} aria-hidden="true" />
        <span className="ct-run-row__main">{eventTime(run.at)}</span>
        <span className="ct-run-row__meta">{run.state}</span>
      </button>)}
    </div>}
    {loadHistory && <button type="button" aria-expanded={showDiff} onClick={() => setShowDiff(!showDiff)}>Semantic diff</button>}
    {showDiff && loadHistory && <BlueprintSemanticDiff load={loadHistory} />}
    {onStop && <button type="button" disabled={stopping || !selected || !canStopPipelineRun(pipeline, selected)} onClick={() => void stopRun()}>Stop selected run</button>}
    {error && <p role="alert">{error}</p>}
    {stop && <section aria-label="Recorded stop"><dl><dt>Node</dt><dd>{stop.step_name}</dd><dt>Reason</dt><dd>{stop.reason ?? "unavailable"}</dd></dl><p aria-label="Stop call stack">{stop.call_stack?.map((frame) => `${frame.node}: ${frame.id}@${frame.hash}`).join(" / ")}</p></section>}
    {selected?.nested_run?.id === selected?.runId && selected?.nested_run && <section aria-label="Selected run journal"><ol>{selected.nested_run.nodes.map((node, index) => <li key={index}><span>{node.step_name} · {node.state}</span><p>{node.call_stack?.map((frame) => `${frame.node}: ${frame.id}@${frame.hash}`).join(" / ")}</p><BlueprintProjectionValue value={node} label={`Journal entry ${index + 1}`} /></li>)}</ol></section>}
    {pipeline.note && <p className="ct-meta-muted">{pipeline.note}</p>}
    {selected ? <PipelineChain key={selected.runId ?? "definition"} pipeline={selected} /> : <p role="status">Selected run unavailable.</p>}
  </div>;
}

function PipelineChain({ pipeline }: { pipeline: PipelineView }) {
  const markerId = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const [expanded, setExpanded] = useState<BlueprintExpansion>(new Set());
  const [selected, setSelected] = useState<string | null>(null);
  const { graph, error } = useBlueprintGraph(pipeline, expanded);
  if (pipeline.historical_graph_available === false) return <p role="status" aria-label="Historical graph unavailable">Historical expanded graph unavailable. The retained journal is shown above.</p>;
  if (error || pipeline.projection_error) return <p role="alert">{error ?? pipeline.projection_error}</p>;
  if (!graph) return <p role="status">Building Blueprint graph…</p>;
  const { layout, model } = graph;
  const byKey = new Map(model.nodes.map((node) => [node.key, node]));
  const selectedNode = model.nodes.find((node) => node.key === selected);
  const callFor = (node: PipelineViewNode) => node.definition ? expandedCall(pipeline, node.callPath ?? [], node.definition.id) : undefined;
  return <><div role="group" aria-label={`${pipeline.name} pipeline`} className="bp-canvas min-w-0 overflow-x-auto rounded-[14px]">
    <div className="relative" style={{ width: layout.width, height: layout.height, minHeight: PIPELINE_CANVAS_HEIGHT }}>
      <svg className="pointer-events-none absolute inset-0" width={layout.width} height={layout.height} aria-hidden="true">
        <defs><marker id={`pipeline-arrow-${markerId}`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M 0 0 L 10 5 L 0 10 z" fill="var(--ct-muted-fg)" /></marker></defs>
        {layout.edges.map((edge, index) => <path key={index} d={edge.path} stroke="var(--ct-muted-fg)" strokeWidth="1.5" markerEnd={`url(#pipeline-arrow-${markerId})`} />)}
      </svg>
      {layout.edges.map((edge) => edge.label ? <span key={`label:${edge.from}:${edge.to}:${edge.label}`} title={edge.label} className="absolute z-10 max-w-[4.5rem] -translate-x-1/2 truncate rounded-full bg-[color-mix(in_srgb,var(--ct-fg)_6%,var(--ct-bg))] px-1.5 py-0.5 text-[0.625rem] text-[var(--ct-muted-fg)]" style={{ left: edge.x, top: edge.y - 28 }}>{edge.label}</span> : null)}
      {layout.nodes.map((box) => {
        const node = byKey.get(box.key);
        if (!node) return null;
        return <div key={box.key} className="absolute" style={{ left: box.x, top: box.y, width: Math.min(box.width, PIPELINE_CARD_WIDTH), height: Math.min(box.height, PIPELINE_CARD_HEIGHT) }}>
          <button type="button" className="h-full w-full text-left" aria-label={`Node ${node.definition?.id ?? node.title}${node.callPath?.length ? ` in ${node.callPath.join(" / ")}` : ""}`} aria-pressed={selected === node.key} onClick={() => setSelected(node.key)}><PipelineNodeCard node={node} /></button>
        </div>;
      })}
    </div>
  </div>
    <ul aria-label="Blueprint routes" className="sr-only">{model.edges.map((edge, index) => <li key={index}>{byKey.get(edge.from)?.definition?.id ?? byKey.get(edge.from)?.title} → {byKey.get(edge.to)?.definition?.id ?? byKey.get(edge.to)?.title}: {edge.label}</li>)}</ul>
    {selectedNode && <BlueprintPipelineNodeDetails node={selectedNode} call={callFor(selectedNode)} expanded={expanded.has(selectedNode.key)} onExpand={() => setExpanded((current) => toggleBlueprintExpansion(current, selectedNode.key))} onClose={() => setSelected(null)} />}
  </>;
}
