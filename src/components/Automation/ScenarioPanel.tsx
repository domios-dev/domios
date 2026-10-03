import { useMemo, useRef, useState, type ReactNode } from "react";
import { ArrowLeft, Bot, Clock3, List, Network, Radar } from "lucide-react";
import { AutomationCanvas } from "./AutomationCanvas";
import { AutomationDetails, AutomationDetailsUnavailable } from "./AutomationDetails";
import { automationState, scopedAutomationGraph } from "./model";
import { sentinelJobType } from "./scenarios";
import { AUTOMATION_ROW, AUTOMATION_TOOL, stateTone } from "./styles";
import { automationKey, type AutomationGraph, type AutomationIdentity, type AutomationNode, type AutomationScenario, type AutomationScope } from "./types";
import type { AutomationCamera } from "./useAutomationViewport";

const KIND_ICON = { sentinel: Radar, cron: Clock3, worker: Bot } as const;

export interface ScenarioState {
  graph: AutomationGraph | null;
  failed: boolean;
  stopping: string | null;
  stopFailed: string | null;
  stop: (identity: AutomationIdentity) => Promise<void>;
}

function Legend() {
  return <div className="flex flex-wrap items-center gap-x-5 gap-y-1 px-4 py-2 text-xs text-[var(--ct-muted-fg)]">
    <span className="flex items-center gap-2"><svg width="28" height="6" aria-hidden="true"><line x1="0" y1="3" x2="28" y2="3" stroke="var(--ct-muted-fg)" strokeWidth="1.5" strokeDasharray="6 5" /></svg>Dashed: configured route</span>
    <span className="flex items-center gap-2"><svg width="28" height="6" aria-hidden="true"><line x1="0" y1="3" x2="28" y2="3" stroke="var(--ct-fg)" strokeWidth="2" /></svg>Solid: recorded execution</span>
  </div>;
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
        <span className="block truncate text-xs text-[var(--ct-muted-fg)]">{entry.identity.kind}{entry.hidden ? " · Hidden" : ""} · {links} {links === 1 ? "link" : "links"}</span>
      </span>
      <span className="flex shrink-0 items-center gap-1.5 text-xs text-[var(--ct-muted-fg)]"><span className={`h-1.5 w-1.5 rounded-full ${stateTone(state)}`} aria-hidden="true" />{state}</span>
    </button></li>;
  })}</ul></div>;
}

export function ScenarioPanel({ state, scope, scenario, camera, backLabel, onBack, headingLevel = 2, children }: {
  state: ScenarioState; scope: AutomationScope; scenario: AutomationScenario; camera: AutomationCamera;
  backLabel: string; onBack: () => void; headingLevel?: 2 | 3; children?: ReactNode;
}) {
  const [list, setList] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const opener = useRef<HTMLElement | null>(null);
  const body = useRef<HTMLDivElement>(null);
  const { workspace_id, team_id } = scope;
  const graph = useMemo(() => state.graph ? scopedAutomationGraph({ ...state.graph, scenarios: [scenario] }, { workspace_id, team_id }) : null, [state.graph, scenario, workspace_id, team_id]);
  const node = graph?.nodes.find((candidate) => automationKey(candidate.identity) === selected);
  const select = (next: AutomationNode) => {
    if (document.activeElement instanceof HTMLElement) opener.current = document.activeElement;
    setSelected(automationKey(next.identity));
  };
  const closeDetails = () => {
    setSelected(null);
    if (opener.current?.isConnected) opener.current.focus();
    else body.current?.querySelector<HTMLElement>('[role="region"], ul button')?.focus();
  };
  const Heading = headingLevel === 3 ? "h3" : "h2";
  const ToggleIcon = list ? Network : List;
  return <div className="min-w-0">
    <div className="flex flex-wrap items-center gap-2 border-b border-[var(--ct-border)] px-4 py-3">
      <button type="button" className={AUTOMATION_TOOL} onClick={onBack}><ArrowLeft size={14} aria-hidden="true" />{backLabel}</button>
      <Heading className="min-w-0 flex-1 truncate text-[length:var(--ct-font-size-md)] font-semibold text-[var(--ct-fg)]"><span>{scenario.name}</span><span className="font-normal text-[var(--ct-muted-fg)]"> · {sentinelJobType(graph?.nodes ?? [])}</span></Heading>
      <button type="button" className={AUTOMATION_TOOL} aria-pressed={list} onClick={() => setList(!list)}><ToggleIcon size={14} aria-hidden="true" />{list ? "Show graph" : "Show list"}</button>
    </div>
    {children}
    {graph && <div ref={body}>
      {graph.nodes.length === 0 && <p role="alert" className="px-4 py-3">The Sentinel root is unavailable.</p>}
      {!list && <Legend />}
      {list ? <ParticipantList graph={graph} selected={selected} onSelect={select} /> : <AutomationCanvas graph={graph} selected={selected} camera={camera} onSelect={select} />}
      {node
        ? <AutomationDetails node={node} nodes={graph.nodes} edges={graph.edges} stale={state.failed} stopping={state.stopping === selected} stopFailed={state.stopFailed === selected} onStop={() => { void state.stop(node.identity); }} onSelect={select} onClose={closeDetails} />
        : selected !== null && <AutomationDetailsUnavailable onClose={closeDetails} />}
    </div>}
  </div>;
}
