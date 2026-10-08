import { useMemo, useRef, useState, type ReactNode } from "react";
import { ArrowLeft, List } from "lucide-react";
import { AutomationCanvas } from "./AutomationCanvas";
import { RunLog } from "./RunLog";
import { replayEdges } from "./runLogModel";
import { projectAutomationGraph, type ActionLeaf } from "./projection";
import type { ReplaySelection } from "./useRunReplay";
import { AutomationDetails } from "./AutomationDetails";
import { usePresence, type PresenceMotion } from "../Canvas/usePresence";
import { automationState, scopedAutomationGraph } from "./model";
import { sentinelJobType, sentinelTitle } from "./scenarios";
import { AUTOMATION_TOOL } from "./styles";
import { ALL_JOBS_SCOPE, automationKey, type AutomationGraph, type AutomationIdentity, type AutomationNode, type AutomationScenario, type AutomationScope } from "./types";
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

/** `fill` stretches the scenario over a bounded parent's remaining height; embedded consumers keep content height. */
export function ScenarioPanel({ state, scope, scenario, camera, backLabel, onBack, headingLevel = 2, fill = false, children }: {
  state: ScenarioState; scope: AutomationScope; scenario: AutomationScenario; camera: AutomationCamera;
  backLabel: string; onBack: () => void; headingLevel?: 2 | 3; fill?: boolean; children?: ReactNode;
}) {
  const [runsOpen, setRunsOpen] = useState(false);
  const [replay, setReplay] = useState<ReplaySelection | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const opener = useRef<HTMLElement | null>(null);
  const body = useRef<HTMLDivElement>(null);
  const drawer = usePresence<string>(selected, DRAWER_MOTION);
  const { workspace_id, team_id } = scope;
  const graph = useMemo(() => {
    if (!state.graph) return null;
    const scenarioTeam = team_id === ALL_JOBS_SCOPE ? scenario.root.team_id : team_id;
    const source = state.graph.all_teams ? { ...state.graph, all_teams: false, team_id: scenarioTeam } : state.graph;
    return scopedAutomationGraph({ ...source, scenarios: [scenario] }, { workspace_id, team_id: scenarioTeam });
  }, [state.graph, scenario, workspace_id, team_id]);
  const title = sentinelTitle(scenario, graph?.nodes.find((entry) => automationKey(entry.identity) === automationKey(scenario.root)) ?? null);
  const shown = drawer.item;
  const action = graph ? projectAutomationGraph(graph).nodes.find((candidate) => candidate.key === shown)?.action ?? null : null;
  const node = action?.source ?? graph?.nodes.find((candidate) => automationKey(candidate.identity) === shown) ?? null;
  const selectKey = (key: string) => {
    const active = document.activeElement;
    // A participant opened from inside the drawer keeps the original opener so closing returns behind the drawer.
    if (active instanceof HTMLElement && !drawer.ref.current?.contains(active)) opener.current = active;
    setSelected(key);
  };
  const select = (next: AutomationNode) => selectKey(automationKey(next.identity));
  const selectAction = (next: ActionLeaf) => selectKey(next.key);
  const closeDetails = () => {
    setSelected(null);
    if (opener.current?.isConnected && !drawer.ref.current?.contains(opener.current)) opener.current.focus();
    else body.current?.querySelector<HTMLElement>('[role="region"], ul button')?.focus();
  };
  const Heading = headingLevel === 3 ? "h3" : "h2";
  const root = graph?.nodes.find((candidate) => automationKey(candidate.identity) === automationKey(scenario.root));
  return <div className={`flex min-w-0 flex-col ${fill ? "min-h-0 flex-1" : ""}`}>
    <div className="flex flex-wrap items-center gap-2 border-b border-[var(--ct-border)] px-4 py-3">
      <button type="button" className={AUTOMATION_TOOL} onClick={onBack}><ArrowLeft size={14} aria-hidden="true" />{backLabel}</button>
      <Heading className="min-w-0 flex-1 truncate text-[length:var(--ct-font-size-md)] font-semibold text-[var(--ct-fg)]"><span>{title}</span><span className="font-normal text-[var(--ct-muted-fg)]"> · {sentinelJobType(graph?.nodes ?? [])}</span></Heading>
      {root && <span className="text-xs text-[var(--ct-muted-fg)]">{automationState(root)}{root.schedule ? ` · ${root.schedule}` : root.cadence_ms ? ` · every ${root.cadence_ms / 1000}s` : ""}</span>}
      {root && <button type="button" className={AUTOMATION_TOOL} disabled={!root.stop.allowed || state.failed || state.stopping !== null} onClick={() => void state.stop(root.identity)}>{state.stopping ? "Stopping…" : "Stop Blueprint"}</button>}
      <button type="button" className={AUTOMATION_TOOL} aria-expanded={runsOpen} aria-controls="blueprint-run-log" onClick={() => setRunsOpen(!runsOpen)}><List size={14} aria-hidden="true" />Runs</button>
    </div>
    {children}
    {state.stopFailed && <p role="alert" className="px-4 py-2">Blueprint stop failed.</p>}
    {graph && <div ref={body} className={`relative flex min-w-0 flex-col ${fill ? "min-h-0 flex-1" : drawer.state ? "min-h-[28rem]" : ""}`}>
      {graph.nodes.length === 0 && <p role="alert" className="px-4 py-3">The Blueprint root is unavailable.</p>}
      <AutomationCanvas graph={graph} selected={selected} camera={camera} fill={fill} onSelect={select} onSelectAction={selectAction} replay={replay} />
      {runsOpen && <RunLog graph={graph} selected={replay?.key ?? null} onReplay={(run) => setReplay({ key: run.key, edges: replayEdges(run) })} />}
      {drawer.state && shown !== null && <div className="plan-side-panel-layer">
        <AutomationDetails node={node} action={action} nodes={graph.nodes} onClose={closeDetails} panelRef={drawer.ref} presence={drawer.state} />
      </div>}
    </div>}
  </div>;
}
