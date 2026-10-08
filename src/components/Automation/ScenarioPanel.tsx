import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { List, Square, X } from "lucide-react";
import { BlueprintExportButton } from "./BlueprintExportButton";
import { BlueprintCanvas, type CanvasReplay } from "./BlueprintCanvas";
import { BlueprintNodeDetails } from "./BlueprintNodeDetails";
import { layoutBlueprint } from "./blueprintLayout";
import { blueprintModel } from "./blueprintModel";
import { replayFrame, replaySteps, runRow } from "./blueprintRunModel";
import { clampLogHeight, readLogHeight, writeLogHeight } from "./dividerModel";
import { RunDivider } from "./RunDivider";
import { RunLog, runTime } from "./RunLog";
import { graphRuns } from "./runLogModel";
import { usePresence, type PresenceMotion } from "../Canvas/usePresence";
import { automationState, scopedAutomationGraph } from "./model";
import { sentinelTitle } from "./scenarios";
import { AUTOMATION_TOOL, stateTone } from "./styles";
import { ALL_JOBS_SCOPE, automationKey, type AutomationGraph, type AutomationIdentity, type AutomationScenario, type AutomationScope } from "./types";
import { useBlueprintReplay } from "./useBlueprintReplay";
import type { AutomationCamera } from "./useAutomationViewport";

export interface ScenarioState {
  graph: AutomationGraph | null;
  failed: boolean;
  stopping: string | null;
  stopFailed: string | null;
  stop: (identity: AutomationIdentity) => Promise<void>;
}

/** Shares the app's right side-panel motion so every detail drawer opens the same calm way. */
const DRAWER_MOTION: PresenceMotion = { enter: "ct-plan-side-panel-in", exit: "ct-plan-side-panel-out" };

function storage(): Storage | null {
  try { return typeof window === "undefined" ? null : window.localStorage; } catch { return null; }
}

function useMeasuredHeight<T extends HTMLElement>() {
  const [element, setElement] = useState<T | null>(null);
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

/** `fill` stretches the scenario over a bounded parent's remaining height; embedded consumers get a bounded canvas. */
export function ScenarioPanel({ state, scope, scenario, camera, backLabel, onBack, headingLevel = 2, fill = false, initialRunId, children }: {
  state: ScenarioState; scope: AutomationScope; scenario: AutomationScenario; camera: AutomationCamera;
  backLabel: string; onBack: () => void; headingLevel?: 2 | 3; fill?: boolean; initialRunId?: string; children?: ReactNode;
}) {
  const [runsOpen, setRunsOpen] = useState(true);
  const [replay, setReplay] = useState<{ key: string; nonce: number } | null>(initialRunId ? { key: initialRunId, nonce: 0 } : null);
  const [selected, setSelected] = useState<string | null>(camera.selected);
  const [logHeight, setLogHeight] = useState(() => readLogHeight(storage()));
  const body = useMeasuredHeight<HTMLDivElement>();
  const opener = useRef<HTMLElement | null>(null);
  const drawer = usePresence<string>(selected, DRAWER_MOTION);
  const [panelWidth, setPanelWidth] = useState(0);
  useEffect(() => { camera.selected = selected; }, [camera, selected]);
  useLayoutEffect(() => {
    const panel = drawer.ref.current;
    if (!drawer.state || !panel) { setPanelWidth(0); return; }
    const update = () => setPanelWidth(panel.getBoundingClientRect().width);
    update();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(update);
    observer.observe(panel);
    return () => observer.disconnect();
  }, [drawer.state, drawer.ref]);
  const { workspace_id, team_id } = scope;
  const graph = useMemo(() => {
    if (!state.graph) return null;
    const scenarioTeam = team_id === ALL_JOBS_SCOPE ? scenario.root.team_id : team_id;
    const source = state.graph.all_teams ? { ...state.graph, all_teams: false, team_id: scenarioTeam } : state.graph;
    return scopedAutomationGraph({ ...source, scenarios: [scenario] }, { workspace_id, team_id: scenarioTeam });
  }, [state.graph, scenario, workspace_id, team_id]);
  const model = useMemo(() => graph ? blueprintModel(graph) : null, [graph]);
  const layout = useMemo(() => model ? layoutBlueprint(model) : null, [model]);
  const rows = useMemo(() => graph && model ? graphRuns(graph).map((run) => runRow(model, run)) : [], [graph, model]);
  const replayRow = replay ? rows.find((row) => row.key === replay.key) ?? null : null;
  const steps = useMemo(() => model && replayRow ? replaySteps(model, replayRow.run) : null, [model, replayRow]);
  const frame = useBlueprintReplay(steps, replay?.nonce ?? 0);
  const liveRow = rows.find((row) => row.status === "running");
  const live = useMemo(() => model && liveRow ? replayFrame(replaySteps(model, liveRow.run), 0, true) : null, [model, liveRow]);
  const canvasReplay: CanvasReplay | null = frame ? { frame, live: false } : live ? { frame: live, live: true } : null;

  const root = graph?.nodes.find((candidate) => automationKey(candidate.identity) === automationKey(scenario.root));
  const title = sentinelTitle(scenario, root ?? null);
  const shown = drawer.item;
  const shownNode = shown && model ? model.nodes.find((node) => node.key === shown) ?? null : null;
  const select = (key: string) => {
    const active = document.activeElement;
    if (active instanceof HTMLElement && !drawer.ref.current?.contains(active)) opener.current = active;
    setSelected(key);
  };
  const closeDetails = () => {
    setSelected(null);
    if (opener.current?.isConnected && !drawer.ref.current?.contains(opener.current)) opener.current.focus();
    else body.element?.querySelector<HTMLElement>('[role="region"]')?.focus();
  };
  const available = body.height || 640;
  const resizeLog = (height: number) => {
    const next = clampLogHeight(height, available);
    setLogHeight(next);
    writeLogHeight(storage(), next);
  };
  const Heading = headingLevel === 3 ? "h3" : "h2";
  const state_ = root ? automationState(root) : null;
  const cadence = root?.schedule ?? (root?.cadence_ms ? `every ${root.cadence_ms / 1000} s` : null);
  const replayBar = replay && frame && replayRow ? <div className="ml-auto flex min-w-[12rem] items-center gap-2.5" role="group" aria-label="Replay">
    <span className="whitespace-nowrap text-xs text-[var(--ct-fg)]">{frame.done ? "Replayed" : "Replaying"} run {runTime(replayRow.at)}</span>
    <span className="bp-replay-bar w-24" role="progressbar" aria-label="Replay progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(frame.overall * 100)}><span style={{ width: `${frame.overall * 100}%` }} /></span>
    <button type="button" className={AUTOMATION_TOOL} onClick={() => setReplay(null)}><Square size={12} aria-hidden="true" />{frame.done ? "Clear" : "Stop"}</button>
  </div> : null;

  return <div className={`flex min-w-0 flex-col ${fill ? "min-h-0 flex-1" : ""}`} onKeyDown={(event) => {
    if (event.code !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    onBack();
  }}>
    <div className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-[var(--ct-border)] py-2.5 pl-4 pr-3">
      <Heading className="min-w-0 flex-[1_1_12rem] truncate text-[length:var(--ct-font-size-md)] font-semibold text-[var(--ct-fg)]">{title}</Heading>
      {state_ && <span className="flex items-center gap-1.5 text-xs text-[var(--ct-muted-fg)]"><span className={`h-1.5 w-1.5 rounded-full ${stateTone(state_)}`} aria-hidden="true" />{state_}{cadence ? ` · ${cadence}` : ""}</span>}
      <BlueprintExportButton workspaceId={scope.workspace_id} teamId={scenario.root.team_id} id={scenario.root.id} />
      {root && <button type="button" className={AUTOMATION_TOOL} disabled={!root.stop.allowed || state.failed || state.stopping !== null} onClick={() => void state.stop(root.identity)}>{state.stopping ? "Stopping…" : "Stop Blueprint"}</button>}
      <button type="button" className={AUTOMATION_TOOL} aria-expanded={runsOpen} aria-controls="blueprint-run-log" onClick={() => setRunsOpen(!runsOpen)}><List size={14} aria-hidden="true" />Runs</button>
      <button type="button" className="plan-side-panel__action" aria-label="Close Blueprint" title={`Close and return to ${backLabel} (Esc)`} onClick={onBack}><X size={15} aria-hidden="true" /></button>
    </div>
    {children}
    {state.stopFailed && <p role="alert" className="px-4 py-2">Blueprint stop failed.</p>}
    {graph && model && layout && <div ref={body.ref} className={`relative flex min-w-0 flex-col ${fill ? "min-h-0 flex-1" : "h-[min(44rem,78vh)] min-h-[28rem]"}`}>
      {model.stages.length === 0 && <p role="alert" className="px-4 py-3">The Blueprint root is unavailable.</p>}
      <div className="flex min-h-0 min-w-0 flex-1 flex-col" style={{ marginRight: panelWidth }}>
        <BlueprintCanvas layout={layout} selected={selected} camera={camera} onSelect={select} replay={canvasReplay} panelOpen={selected !== null} toolbar={replayBar} />
        {runsOpen && <>
          <RunDivider height={clampLogHeight(logHeight, available)} available={available} controls="blueprint-run-log" onResize={resizeLog} />
          <RunLog rows={rows} model={model} selected={replay?.key ?? null} truncated={graph.nodes.some((node) => node.history_truncated)} height={clampLogHeight(logHeight, available)} onReplay={(row) => setReplay((current) => ({ key: row.key, nonce: (current?.nonce ?? 0) + 1 }))} />
        </>}
      </div>
      {drawer.state && shown !== null && <div className="plan-side-panel-layer">
        <BlueprintNodeDetails node={shownNode} graph={graph} onClose={closeDetails} panelRef={drawer.ref} presence={drawer.state} />
      </div>}
    </div>}
  </div>;
}
