import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { List, Network, Square } from "lucide-react";
import { BackPill } from "../shared/BackPill";
import { BlueprintExportButton } from "./BlueprintExportButton";
import { BlueprintCanvas, type CanvasReplay } from "./BlueprintCanvas";
import { BlueprintNodeDetails } from "./BlueprintNodeDetails";
import { layoutBlueprint } from "./blueprintLayout";
import { blueprintModel } from "./blueprintModel";
import { replayFrame, replaySteps, runRow } from "./blueprintRunModel";
import { clampLogHeight, readLogHeight, writeLogHeight } from "./dividerModel";
import { RunDivider } from "./RunDivider";
import { RunLog } from "./RunLog";
import { graphRuns } from "./blueprintRunModel";
import { usePresence, type PresenceMotion } from "../Canvas/usePresence";
import { automationState, scopedAutomationGraph } from "./model";
import { blueprintTitle } from "./scenarios";
import { AUTOMATION_TONE, AUTOMATION_TOOL, stateTone } from "./styles";
import { ALL_JOBS_SCOPE, automationKey, type AutomationGraph, type AutomationIdentity, type AutomationScenario, type AutomationScope } from "./types";
import { useBlueprintReplay } from "./useBlueprintReplay";
import { BlueprintReplayStatus } from "./BlueprintReplayStatus";
import { useBlueprintReplaySelection } from "./useBlueprintReplaySelection";
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
  const replaySelection = useBlueprintReplaySelection(model, rows, initialRunId);
  const replay = replaySelection.selection;
  const replayRow = replaySelection.shown?.row ?? null;
  const steps = replaySelection.shown?.steps ?? null;
  const frame = useBlueprintReplay(steps, replay?.nonce ?? 0);
  const outcome = replaySelection.shown?.outcome ?? null;
  const liveRow = rows.find((row) => row.status === "running");
  const live = useMemo(() => model && liveRow ? replayFrame(replaySteps(model, liveRow.run), 0, true) : null, [model, liveRow]);
  const canvasReplay: CanvasReplay | null = frame ? { frame, live: false, outcome } : live ? { frame: live, live: true } : null;

  const root = graph?.nodes.find((candidate) => automationKey(candidate.identity) === automationKey(scenario.root));
  const title = blueprintTitle(scenario, root ?? null);
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
    <BlueprintReplayStatus done={frame.done} at={replayRow.at} outcome={outcome} />
    <span className="bp-replay-bar w-24" role="progressbar" aria-label="Replay progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(frame.overall * 100)}><span style={{ width: `${frame.overall * 100}%` }} /></span>
    <button type="button" className={AUTOMATION_TOOL} onClick={replaySelection.clear}><Square size={12} aria-hidden="true" />{frame.done ? "Clear" : "Stop"}</button>
  </div> : null;

  return <div className={`flex min-w-0 flex-col ${fill ? "min-h-0 flex-1" : ""}`} onKeyDown={(event) => {
    if (event.code !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    onBack();
  }}>
    <div className="ct-jobs-header flex flex-wrap items-center gap-x-3 gap-y-2 py-2.5 pl-4 pr-3">
      <BackPill label={backLabel} onBack={onBack} />
      <span className="ct-card__icon" style={{ "--ct-card-tone": AUTOMATION_TONE.blueprint } as CSSProperties}><Network size={14} aria-hidden="true" /></span>
      <Heading className="min-w-0 flex-[1_1_12rem] break-words text-[length:var(--ct-font-size-md)] font-semibold text-[var(--ct-fg)]">{title}</Heading>
      {state_ && <span className="flex items-center gap-1.5 text-xs text-[var(--ct-muted-fg)]"><span className={`h-1.5 w-1.5 rounded-full ${stateTone(state_)}`} aria-hidden="true" />{state_}{cadence ? ` · ${cadence}` : ""}</span>}
      <BlueprintExportButton workspaceId={scope.workspace_id} teamId={scenario.root.team_id} id={scenario.root.id} />
      {root && <button type="button" className={AUTOMATION_TOOL} disabled={!root.stop.allowed || state.failed || state.stopping !== null} onClick={() => void state.stop(root.identity)}>{state.stopping ? "Stopping…" : "Stop Blueprint"}</button>}
      <button type="button" className={AUTOMATION_TOOL} aria-expanded={runsOpen} aria-controls="blueprint-run-log" onClick={() => setRunsOpen(!runsOpen)}><List size={14} aria-hidden="true" />Runs</button>
    </div>
    {children}
    {state.stopFailed && <p role="alert" className="ct-job-note px-4 py-2">Blueprint stop failed.</p>}
    {graph && model && layout && <div ref={body.ref} className={`relative flex min-w-0 flex-col ${fill ? "min-h-0 flex-1" : "h-[min(44rem,78vh)] min-h-[28rem]"}`}>
      {model.stages.length === 0 && <p role="alert" className="px-4 py-3">The Blueprint root is unavailable.</p>}
      <div className="flex min-h-0 min-w-0 flex-1 flex-col" style={{ marginRight: panelWidth }}>
        <BlueprintCanvas layout={layout} selected={selected} camera={camera} onSelect={select} replay={canvasReplay} panelOpen={selected !== null} toolbar={replayBar} />
        {runsOpen && <>
          <RunDivider height={clampLogHeight(logHeight, available)} available={available} controls="blueprint-run-log" onResize={resizeLog} />
          <RunLog rows={rows} model={model} selected={replay?.key ?? null} truncated={graph.nodes.some((node) => node.history_truncated)} height={clampLogHeight(logHeight, available)} onReplay={replaySelection.replay} />
        </>}
      </div>
      {drawer.state && shown !== null && <div className="plan-side-panel-layer">
        <BlueprintNodeDetails node={shownNode} graph={graph} onClose={closeDetails} panelRef={drawer.ref} presence={drawer.state} />
      </div>}
    </div>}
  </div>;
}
