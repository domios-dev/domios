import { useRef, useState } from "react";
import { AutomationCanvas } from "./AutomationCanvas";
import { AUTOMATION_BUTTON } from "./styles";
import { AutomationDetails } from "./AutomationDetails";
import { automationState } from "./model";
import { automationKey, type AutomationAttachment, type AutomationGraphAdapter, type AutomationNode } from "./types";
import { useAutomationGraph } from "./useAutomationGraph";

export interface AutomationGraphViewProps {
  adapter: AutomationGraphAdapter;
  attachment: AutomationAttachment;
}

function AutomationGraphSurface({ adapter, attachment }: AutomationGraphViewProps) {
  const state = useAutomationGraph(adapter, attachment);
  const [list, setList] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const opener = useRef<HTMLElement | null>(null);
  const select = (node: AutomationNode) => {
    if (document.activeElement instanceof HTMLElement) opener.current = document.activeElement;
    setSelected(automationKey(node.identity));
  };
  const node = state.graph?.nodes.find((candidate) => automationKey(candidate.identity) === selected);
  return <section aria-label="Automations" className="min-w-0 text-[var(--ct-fg)]">
    <div className="flex items-center justify-between gap-3 border-b border-[var(--ct-border)] p-3">
      <h2 className="text-sm font-semibold">Automations</h2>
      <button type="button" className={AUTOMATION_BUTTON} aria-pressed={list} onClick={() => setList(!list)}>{list ? "Show graph" : "Show list"}</button>
    </div>
    <p className="p-3 text-xs text-[var(--ct-muted-fg)]">Dashed: configured routes. Solid: recorded executions.</p>
    {state.loading && <p role="status" className="p-3">Reading automations…</p>}
    {state.failed && <p role="alert" className="p-3">Automations unavailable. Retained records are stale; stopping is disabled.</p>}
    {state.graph && <>
      {state.graph.nodes.length === 0 && <p className="p-3">No automations recorded in this scope.</p>}
      {list ? <ul aria-label="Automation participants" className="space-y-2 p-3">{state.graph.nodes.map((entry) => <li key={automationKey(entry.identity)}><button type="button" className={AUTOMATION_BUTTON} aria-pressed={selected === automationKey(entry.identity)} onClick={() => select(entry)}>{entry.identity.kind} · {entry.name} · {automationState(entry)}{entry.hidden ? " · Hidden" : ""}</button></li>)}</ul> : <AutomationCanvas graph={state.graph} selected={selected} onSelect={select} />}
      {node && <AutomationDetails node={node} nodes={state.graph.nodes} edges={state.graph.edges} stale={state.failed} stopping={state.stopping === selected} stopFailed={state.stopFailed === selected} onStop={() => { void state.stop(node.identity); }} onSelect={select} onClose={() => { setSelected(null); if (opener.current?.isConnected) opener.current.focus(); }} />}
    </>}
  </section>;
}

export function AutomationGraphView(props: AutomationGraphViewProps) {
  const { connection_id, workspace_id, team_id } = props.attachment;
  return <AutomationGraphSurface key={JSON.stringify([connection_id, workspace_id, team_id])} {...props} />;
}
