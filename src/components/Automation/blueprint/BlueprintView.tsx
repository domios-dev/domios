import { useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { GitCompare, List, Network, Square } from "lucide-react";
import type { BlueprintViewerHistory } from "../../../lib/blueprintApi";
import { usePresence, type PresenceMotion } from "../../Canvas/usePresence";
import { BackPill } from "../../shared/BackPill";
import { InlineErrorNotice } from "../../shared/ErrorNotice";
import { BlueprintExportButton } from "../BlueprintExportButton";
import { BlueprintPlanLock } from "../BlueprintPlanLock";
import { BlueprintProjectionValue } from "../BlueprintProjectionValue";
import { BlueprintSemanticDiff } from "../BlueprintSemanticDiff";
import { blueprintNodeKey, toggleBlueprintExpansion, type BlueprintExpansion } from "../blueprintExpansion";
import { blueprintJournalNodes, blueprintPipelineOutcome, heldReasonLabel } from "../blueprintPipelineOutcome";
import { clampLogHeight, readLogHeight, writeLogHeight } from "../dividerModel";
import type { PipelineView } from "../pipelineModel";
import { RunDivider } from "../RunDivider";
import { AUTOMATION_TONE, AUTOMATION_TOOL, stateTone } from "../styles";
import { BlueprintCanvas, type CanvasCallout } from "./Canvas";
import { CanvasNotices } from "./CanvasNotes";
import type { CanvasNote } from "./layout";
import { DetailsFrame } from "../DetailsFrame";
import { expandedCall, focusScope, litWires } from "./graphModel";
import { layoutBlueprint } from "./layout";
import { BlueprintNodeDetails } from "./NodeDetails";
import { useBlueprintGraph } from "./useBlueprintGraph";
import { createAutomationCamera, type AutomationCamera } from "./viewport";

const DRAWER_MOTION: PresenceMotion = { enter: "ct-plan-side-panel-in", exit: "ct-plan-side-panel-out" };
const cameras = new Map<string, AutomationCamera>();

/** Cameras outlive the view, so a reopened Blueprint keeps its framing and selected node. */
export function blueprintCamera(id: string): AutomationCamera {
  const existing = cameras.get(id);
  if (existing) return existing;
  const created = createAutomationCamera();
  cameras.set(id, created);
  return created;
}

function storage(): Storage | null {
  try { return typeof window === "undefined" ? null : window.localStorage; } catch { return null; }
}

function eventTime(value: number | null | undefined): string {
  return value == null ? "No event recorded" : new Date(value).toLocaleString();
}

function callStack(row: { call_stack?: readonly { node: string; id: string; hash: string }[] }): string {
  return row.call_stack?.map((frame) => `${frame.node}: ${frame.id}@${frame.hash}`).join(" / ") ?? "";
}

function useElementHeight() {
  const [element, setElement] = useState<HTMLDivElement | null>(null);
  const [height, setHeight] = useState(0);
  useLayoutEffect(() => {
    if (!element) return;
    const update = () => setHeight(element.clientHeight);
    update();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [element]);
  return { ref: setElement, element, height };
}

export interface BlueprintExportTarget { workspaceId: string; teamId: string | null; id: string }

/** The one Blueprint view: every pipeline, watch and call opens here as a typed node graph with its runs and journal. */
/** What a host knows about a Blueprint beyond its definition, shown on the canvas note. */
export interface BlueprintAbout { purpose?: string | null; intents?: readonly string[]; facts?: readonly string[] }

export function BlueprintView({ pipeline, initialRunId, backLabel, onBack, exportTarget, actions, about, hostNotices, history = true, onStop, canStop = false, loadHistory, headingLevel = 2, fill = false }: {
  pipeline: PipelineView; initialRunId?: string; backLabel?: string; onBack?: () => void; exportTarget?: BlueprintExportTarget;
  actions?: ReactNode; about?: BlueprintAbout; hostNotices?: ReactNode; history?: boolean;
  onStop?: () => Promise<unknown>; canStop?: boolean; loadHistory?: () => Promise<BlueprintViewerHistory>; headingLevel?: 2 | 3; fill?: boolean;
}) {
  const [trail, setTrail] = useState<readonly string[]>([]);
  const camera = blueprintCamera(trail.length ? `${pipeline.id}|${JSON.stringify(trail)}` : pipeline.id);
  const [runId, setRunId] = useState<string | null>(initialRunId ?? null);
  const [runsOpen, setRunsOpen] = useState(Boolean(initialRunId || pipeline.runs?.length || pipeline.retained || pipeline.runId));
  const [diffOpen, setDiffOpen] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [stopError, setStopError] = useState<unknown>(null);
  const [selected, setSelected] = useState<string | null>(camera.selected);
  const [expanded, setExpanded] = useState<BlueprintExpansion>(new Set());
  const [logHeight, setLogHeight] = useState(() => readLogHeight(storage()));
  const body = useElementHeight();
  const opener = useRef<HTMLElement | null>(null);
  const drawer = usePresence<string>(selected, DRAWER_MOTION);
  const shown = runId === null ? pipeline : pipeline.runs?.find((run) => run.id === runId)?.pipeline ?? null;
  const graphSource = useMemo(() => shown ?? pipeline, [shown, pipeline]);
  const built = useMemo(() => trail.length ? new Set([...expanded, ...trail.map((id, depth) => blueprintNodeKey(trail.slice(0, depth), id))]) : expanded, [expanded, trail]);
  const description = pipeline.note ?? pipeline.call_graph?.intent ?? null;
  const noteKey = JSON.stringify([about?.purpose ?? null, description, about?.intents ?? [], about?.facts ?? []]);
  const note = useMemo<CanvasNote | null>(() => {
    const [purpose, text, intents, facts] = JSON.parse(noteKey) as [string | null, string | null, string[], string[]];
    const body = [...new Set([purpose, text].filter((part): part is string => Boolean(part)))].join("\n\n");
    return body || intents.length || facts.length ? { title: "Note", text: body || null, items: intents, facts } : null;
  }, [noteKey]);
  const { graph: whole, building, error } = useBlueprintGraph(graphSource, built, trail.length ? null : note);
  const graph = useMemo(() => {
    if (!whole || !trail.length) return whole;
    const model = focusScope(whole.model, trail);
    return { ...whole, model, layout: layoutBlueprint(model) };
  }, [whole, trail]);
  const lit = useMemo(() => graph ? litWires(graph.model) : new Set<string>(), [graph]);
  const scopeTitles = [pipeline.name, ...trail.map((id, depth) => expandedCall(graphSource, trail.slice(0, depth), id)?.pin.id ?? id)];
  const enter = (next: readonly string[]) => {
    setTrail(next);
    setSelected(blueprintCamera(next.length ? `${pipeline.id}|${JSON.stringify(next)}` : pipeline.id).selected);
  };
  const drill = (key: string) => {
    const node = graph?.model.nodes.find((entry) => entry.key === key);
    if (node?.type !== "call" || !node.definition || !expandedCall(graphSource, node.callPath, node.definition.id)) return;
    enter([...node.callPath, node.definition.id]);
  };
  const recordedStop = shown ? blueprintPipelineOutcome(shown) : null;
  const callouts = useMemo<CanvasCallout[]>(() => {
    if (!graph) return [];
    const found: CanvasCallout[] = [];
    if (recordedStop) {
      const path = recordedStop.call_stack?.map((frame) => frame.node) ?? [];
      for (let depth = path.length; depth >= 0; depth--) {
        const nodeKey = depth === path.length ? blueprintNodeKey(path, recordedStop.step_name) : blueprintNodeKey(path.slice(0, depth), path[depth]);
        if (!graph.layout.byKey.has(nodeKey)) continue;
        const reason = recordedStop.reason ?? "Reason unavailable";
        found.push({ nodeKey, tone: "failed", label: "Recorded stop", lead: "Stopped here", reason: depth === path.length ? reason : `${recordedStop.step_name}: ${reason}`, detail: recordedStop.call_stack?.length ? callStack(recordedStop) : null });
        break;
      }
    }
    const held = shown === pipeline ? pipeline.heldReason : null;
    if (held) {
      const nodeKey = graph.model.nodes.find((node) => node.evidence?.state === "held")?.key
        ?? (held === "uncertain_pipeline_result" ? blueprintNodeKey([], "$result") : null);
      if (nodeKey && graph.layout.byKey.has(nodeKey)) found.push({ nodeKey, tone: "held", label: "Held", lead: "Held here", reason: heldReasonLabel(held), detail: null });
    }
    return found;
  }, [recordedStop, graph, shown, pipeline]);
  const anchored = (tone: CanvasCallout["tone"]) => callouts.some((callout) => callout.tone === tone);
  const heldNotice = pipeline.heldReason && !anchored("held") ? <p role="status" className="bp-notice" data-tone="held">Held: {heldReasonLabel(pipeline.heldReason)}</p> : null;
  const evidenceNotice = graph?.model.mode === "unavailable" ? <p role="status" className="bp-notice">Per-node evidence is unavailable for this run.</p> : null;
  const planLock = pipeline.heldReason === "plan_limit_locked" ? <span className="bp-notice"><BlueprintPlanLock reason={pipeline.heldReason} /></span> : null;
  const notices = <>
    {building && graph && <p role="status" className="bp-notice">Updating graph…</p>}
    {planLock}{heldNotice}{evidenceNotice}{hostNotices}
    {stopError != null && <InlineErrorNotice error={stopError} operation="stopping this Blueprint" blocking />}
    {recordedStop && !anchored("failed") && <section aria-label="Recorded stop" className="bp-notice" data-tone="failed">
      <span className="font-semibold">Stopped at {recordedStop.step_name}</span>
      <span>{recordedStop.reason ?? "Reason unavailable"}</span>
      {recordedStop.call_stack?.length ? <span aria-label="Stop call stack" className="bp-mono">{callStack(recordedStop)}</span> : null}
    </section>}
  </>;
  const crumbs = trail.length > 0 ? <nav aria-label="Blueprint path" className="bp-crumbs">
    <ol>{scopeTitles.map((title, depth) => <li key={depth}>
      {depth === trail.length ? <span aria-current="page">{title}</span>
        : <button type="button" onClick={() => enter(trail.slice(0, depth))}>{title}</button>}
    </li>)}</ol>
  </nav> : null;
  const shownNode = drawer.item && graph ? graph.model.nodes.find((node) => node.key === drawer.item) ?? null : null;
  const Heading = headingLevel === 3 ? "h3" : "h2";
  const available = body.height || 640;

  const select = (key: string) => {
    const active = document.activeElement;
    if (active instanceof HTMLElement && !drawer.ref.current?.contains(active)) opener.current = active;
    camera.selected = key;
    setDiffOpen(false);
    setSelected(key);
  };
  const closeDetails = () => {
    camera.selected = null;
    setSelected(null);
    if (opener.current?.isConnected && !drawer.ref.current?.contains(opener.current)) opener.current.focus();
    else body.element?.querySelector<HTMLElement>('[role="region"]')?.focus();
  };
  const resizeLog = (height: number) => {
    const next = clampLogHeight(height, available);
    setLogHeight(next);
    writeLogHeight(storage(), next);
  };
  const stop = async () => {
    if (!onStop || stopping) return;
    setStopping(true); setStopError(null);
    try { await onStop(); } catch (failure: unknown) { setStopError(failure); } finally { setStopping(false); }
  };
  const runRow = (pressed: boolean) => `ct-row ct-run-row${pressed ? " ct-row--selected" : ""}`;
  const runCount = pipeline.runs?.length ?? 0;
  const runsToggle = <button type="button" className={AUTOMATION_TOOL} aria-expanded={runsOpen} aria-controls="blueprint-run-log" onClick={() => setRunsOpen(!runsOpen)}><List size={13} aria-hidden="true" />Runs<span className="text-[var(--ct-muted-fg)]">{runCount}</span></button>;
  const journal = shown?.retained
    ? shown.retained.steps.map((step) => ({ name: step.step, state: step.state, detail: [step.outcome, step.summary].filter(Boolean).join(" · "), at: step.at, row: null }))
    : shown?.nested_run && shown.nested_run.id === shown.runId
      ? blueprintJournalNodes(shown).map((row) => ({ name: row.step_name, state: row.state, detail: [row.outcome, row.reason, row.result_summary].filter(Boolean).join(" · "), at: row.ended_at_ms ?? row.started_at_ms ?? null, row }))
      : null;

  return <section aria-label="Blueprint view" className={`bp-view flex min-w-0 flex-col ${fill ? "min-h-0 flex-1" : ""}`} onKeyDown={(event) => {
    if (event.code !== "Escape" || event.defaultPrevented) return;
    if (trail.length) { event.preventDefault(); enter(trail.slice(0, -1)); return; }
    if (!onBack) return;
    event.preventDefault();
    onBack();
  }}>
    <div className="ct-jobs-header flex flex-wrap items-center gap-x-3 gap-y-2 py-2.5 pl-4 pr-3">
      {trail.length ? <BackPill label={scopeTitles[trail.length - 1]} onBack={() => enter(trail.slice(0, -1))} /> : onBack && <BackPill label={backLabel} onBack={onBack} />}
      <span className="ct-card__icon" style={{ "--ct-card-tone": AUTOMATION_TONE.blueprint } as CSSProperties}><Network size={14} aria-hidden="true" /></span>
      <Heading className="min-w-0 flex-[1_1_12rem] break-words text-[length:var(--ct-font-size-md)] font-semibold text-[var(--ct-fg)]">{pipeline.name}</Heading>
      <span className="flex items-center gap-1.5 text-xs text-[var(--ct-muted-fg)]"><span className={`h-1.5 w-1.5 rounded-full ${stateTone(pipeline.state)}`} aria-hidden="true" />{pipeline.call_graph?.kind ?? "Blueprint"} · {pipeline.state}</span>
      {loadHistory && <button type="button" className={AUTOMATION_TOOL} aria-expanded={diffOpen} onClick={() => { if (!diffOpen) { camera.selected = null; setSelected(null); } setDiffOpen(!diffOpen); }}><GitCompare size={13} aria-hidden="true" />Semantic diff</button>}
      {actions}
      {exportTarget && <BlueprintExportButton workspaceId={exportTarget.workspaceId} teamId={exportTarget.teamId} id={exportTarget.id} />}
      {onStop && <button type="button" className={`${AUTOMATION_TOOL} bp-stop`} aria-busy={stopping || undefined} disabled={!canStop || stopping} onClick={() => void stop()}><Square size={12} aria-hidden="true" />{stopping ? "Stopping…" : "Stop Blueprint"}</button>}
    </div>
    <div ref={body.ref} data-blueprint-body="" className={`relative flex min-w-0 flex-col ${fill ? "min-h-0 flex-1" : "h-[min(46rem,80vh)] min-h-[30rem]"}`}>
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        {shown && shown.historical_graph_available !== false && !error && !shown.projection_error && graph
          ? <BlueprintCanvas key={JSON.stringify(trail)} onOpen={drill} layout={graph.layout} label={`${pipeline.name} graph`} selected={selected} camera={camera} onSelect={select} lit={lit} callouts={callouts}
            notices={notices} crumbs={crumbs} panelOpen={selected !== null || diffOpen} toolbar={history ? runsToggle : undefined} />
          : <div className="bp-scope bp-canvas bp-stage relative flex min-h-[14rem] min-w-0 flex-1 flex-col items-start gap-3 p-4">
            {history && runsToggle}
            {!shown ? <p role="status" className="bp-notice">Selected run unavailable.</p>
              : shown.historical_graph_available === false ? <p role="status" aria-label="Historical graph unavailable" className="bp-notice">The graph for this run was not retained. Its journal is listed under Runs.</p>
                : error || shown.projection_error ? <InlineErrorNotice error={error ?? shown.projection_error} operation="building this Blueprint graph" blocking />
                  : <p role="status" className="bp-notice">Building Blueprint graph…</p>}
            <CanvasNotices>{notices}</CanvasNotices>
          </div>}
        {history && runsOpen && <>
          <RunDivider height={clampLogHeight(logHeight, available)} available={available} controls="blueprint-run-log" onResize={resizeLog} />
          <div id="blueprint-run-log" className="bp-runs" style={{ height: clampLogHeight(logHeight, available) }}>
            <div role="group" aria-label="Blueprint runs" className="bp-runs__list">
              <button type="button" className={runRow(runId === null)} aria-label="Latest run" aria-pressed={runId === null} onClick={() => setRunId(null)}>
                <span className="ct-detail__dot" style={{ "--ct-detail-tone": "currentColor" } as CSSProperties} aria-hidden="true" />
                <span className="ct-run-row__main">Latest</span>
                <span className="ct-run-row__meta">{pipeline.state}</span>
              </button>
              {pipeline.runs?.map((run) => <button type="button" key={run.id} className={runRow(runId === run.id)} aria-label={`Run ${run.id}`} aria-pressed={runId === run.id} onClick={() => setRunId(run.id)}>
                <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${stateTone(run.state)}`} aria-hidden="true" />
                <span className="ct-run-row__main">{eventTime(run.at)}</span>
                <span className="ct-run-row__meta">{run.state}</span>
              </button>)}
              {!runCount && <p className="ct-job-note px-2.5 py-2">No retained runs yet.</p>}
            </div>
            <section aria-label="Selected run journal" className="bp-runs__journal">
              <h3 className="bp-panel__title">Run journal</h3>
              {shown?.retained?.incomplete && <p role="status" className="ct-job-note">This retained run is incomplete.</p>}
              {journal?.length ? <ol className="bp-journal">{journal.map((entry, index) => <li key={index} className="bp-journal__entry">
                <div className="bp-journal__head">
                  <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${stateTone(entry.state)}`} aria-hidden="true" />
                  <span className="bp-journal__name">{entry.name}</span>
                  <span className="bp-journal__state">{entry.state}</span>
                  {entry.at != null && <time className="bp-journal__time">{eventTime(entry.at)}</time>}
                </div>
                {entry.detail && <p className="bp-journal__detail">{entry.detail}</p>}
                {entry.row?.call_stack?.length ? <p className="bp-mono">{callStack(entry.row)}</p> : null}
                {entry.row && <BlueprintProjectionValue value={entry.row} label={`Journal entry ${index + 1}`} />}
              </li>)}</ol> : <p className="ct-job-note">{runId === null && !shown?.runId ? "Choose a run to see what each node did." : "No journal entries were retained for this run."}</p>}
            </section>
          </div>
        </>}
      </div>
      {diffOpen && loadHistory && <div className="plan-side-panel-layer">
        <DetailsFrame label="Semantic diff" title={<h3 className="plan-side-panel__title">Semantic diff</h3>} onClose={() => setDiffOpen(false)}>
          <BlueprintSemanticDiff load={loadHistory} />
        </DetailsFrame>
      </div>}
      {!diffOpen && drawer.state && drawer.item !== null && graph && <div className="plan-side-panel-layer">
        <BlueprintNodeDetails key={drawer.item} node={shownNode} model={graph.model}
          call={shownNode?.definition ? expandedCall(graphSource, shownNode.callPath, shownNode.definition.id) : undefined}
          expanded={shownNode ? expanded.has(shownNode.key) : false}
          onExpand={() => shownNode && setExpanded((current) => toggleBlueprintExpansion(current, shownNode.key))}
          onClose={closeDetails} panelRef={drawer.ref} presence={drawer.state} />
      </div>}
    </div>
  </section>;
}
