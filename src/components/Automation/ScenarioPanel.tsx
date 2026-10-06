import { useMemo, useRef, useState, type ReactNode } from "react";
import { ArrowLeft, Bot, Clock3, List, Network, Radar } from "lucide-react";
import { AutomationCanvas } from "./AutomationCanvas";
import { AutomationDetails } from "./AutomationDetails";
import { usePresence, type PresenceMotion } from "../Canvas/usePresence";
import { automationState, scopedAutomationGraph } from "./model";
import { sentinelJobType, sentinelTitle } from "./scenarios";
import { AUTOMATION_ROW, AUTOMATION_TOOL, stateTone } from "./styles";
import { ALL_JOBS_SCOPE, automationKindLabel, automationKey, type AutomationGraph, type AutomationIdentity, type AutomationNode, type AutomationScenario, type AutomationScope } from "./types";
import type { AutomationCamera } from "./useAutomationViewport";

const KIND_ICON = { sentinel: Radar, cron: Clock3, worker: Bot } as const;

export interface ScenarioState {
  graph: AutomationGraph | null;
  failed: boolean;
  stopping: string | null;
  stopFailed: string | null;
  stop: (identity: AutomationIdentity) => Promise<void>;
}

function ParticipantList({ graph, selected, onSelect }: { graph: AutomationGraph; selected: string | null; onSelect: (node: AutomationNode) => void }) {
  return <div className="@container"><ul aria-label="Automation participants" className="grid gap-2 p-3 @2xl:grid-cols-2">{graph.nodes.map((entry) => {
    const key = automationKey(entry.identity);
    const Icon = KIND_ICON[entry.identity.kind];
    const links = graph.edges.filter((edge) => automationKey(edge.source) === key || automationKey(edge.target) === key).length;
    const state = automationState(entry);
    return <li key={key}><button type="button" className={AUTOMATION_ROW} aria-pressed={selected === key} onClick={() => onSelect(entry)}>
      <Icon size={15} className="shrink-0 text-[var(--ct-muted-fg)]" aria-hidden="true" />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[length:var(--ct-font-size-sm)] font-medium">{entry.name}</span>
        <span className="block truncate text-xs text-[var(--ct-muted-fg)]">{automationKindLabel(entry.identity.kind)}{entry.hidden ? " · Hidden" : ""} · {links} {links === 1 ? "link" : "links"}</span>
      </span>
      <span className="flex shrink-0 items-center gap-1.5 text-xs text-[var(--ct-muted-fg)]"><span className={`h-1.5 w-1.5 rounded-full ${stateTone(state)}`} aria-hidden="true" />{state}</span>
    </button></li>;
  })}</ul></div>;
}

/** Shares the app's right side-panel motion so every detail drawer opens the same calm way. */
const DRAWER_MOTION: PresenceMotion = { enter: "ct-plan-side-panel-in", exit: "ct-plan-side-panel-out" };

/** `fill` stretches the scenario over a bounded parent's remaining height; embedded consumers keep content height. */
export function ScenarioPanel({ state, scope, scenario, camera, backLabel, onBack, headingLevel = 2, fill = false, children }: {
  state: ScenarioState; scope: AutomationScope; scenario: AutomationScenario; camera: AutomationCamera;
  backLabel: string; onBack: () => void; headingLevel?: 2 | 3; fill?: boolean; children?: ReactNode;
}) {
  const [list, setList] = useState(false);
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
  const node = graph?.nodes.find((candidate) => automationKey(candidate.identity) === shown) ?? null;
  const select = (next: AutomationNode) => {
    const active = document.activeElement;
    // A participant opened from inside the drawer keeps the original opener so closing returns behind the drawer.
    if (active instanceof HTMLElement && !drawer.ref.current?.contains(active)) opener.current = active;
    setSelected(automationKey(next.identity));
  };
  const closeDetails = () => {
    setSelected(null);
    if (opener.current?.isConnected && !drawer.ref.current?.contains(opener.current)) opener.current.focus();
    else body.current?.querySelector<HTMLElement>('[role="region"], ul button')?.focus();
  };
  const Heading = headingLevel === 3 ? "h3" : "h2";
  const ToggleIcon = list ? Network : List;
  return <div className={`flex min-w-0 flex-col ${fill ? "min-h-0 flex-1" : ""}`}>
    <div className="flex flex-wrap items-center gap-2 border-b border-[var(--ct-border)] px-4 py-3">
      <button type="button" className={AUTOMATION_TOOL} onClick={onBack}><ArrowLeft size={14} aria-hidden="true" />{backLabel}</button>
      <Heading className="min-w-0 flex-1 truncate text-[length:var(--ct-font-size-md)] font-semibold text-[var(--ct-fg)]"><span>{title}</span>{title !== scenario.name && title !== scenario.id && <span className="font-normal text-[var(--ct-muted-fg)]"> · {scenario.id}</span>}<span className="font-normal text-[var(--ct-muted-fg)]"> · {sentinelJobType(graph?.nodes ?? [])}</span></Heading>
      <button type="button" className={AUTOMATION_TOOL} aria-pressed={list} onClick={() => setList(!list)}><ToggleIcon size={14} aria-hidden="true" />{list ? "Show graph" : "Show list"}</button>
    </div>
    {children}
    {graph && <div ref={body} className={`relative flex min-w-0 flex-col ${fill ? "min-h-0 flex-1" : drawer.state ? "min-h-[28rem]" : ""}`}>
      {graph.nodes.length === 0 && <p role="alert" className="px-4 py-3">The Blueprint root is unavailable.</p>}
      {list
        ? <div className={fill ? "min-h-0 flex-1 overflow-y-auto" : undefined}><ParticipantList graph={graph} selected={selected} onSelect={select} /></div>
        : <AutomationCanvas graph={graph} selected={selected} camera={camera} fill={fill} onSelect={select} />}
      {drawer.state && shown !== null && <div className="plan-side-panel-layer">
        <AutomationDetails node={node} nodes={graph.nodes} edges={graph.edges} stale={state.failed} stopping={state.stopping === shown} stopFailed={state.stopFailed === shown}
          onStop={() => { if (node) void state.stop(node.identity); }} onSelect={select} onClose={closeDetails} panelRef={drawer.ref} presence={drawer.state} />
      </div>}
    </div>}
  </div>;
}
