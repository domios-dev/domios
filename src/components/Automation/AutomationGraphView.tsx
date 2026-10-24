import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useLayoutEffect, useRef, useState } from "react";
import { ChevronRight, Network } from "lucide-react";
import { focusEntry, focusLost } from "./focus";
import { HostBlueprintView } from "./blueprint/HostBlueprintView";
import { blueprintScenarioJobs, blueprintTitle } from "./scenarios";
import { AUTOMATION_ROW, stateTone } from "./styles";
import type { AutomationAttachment, AutomationGraphAdapter } from "./types";
import { useAutomationGraph } from "./useAutomationGraph";
import type { AutomationSnapshot } from "./resource";

export interface AutomationGraphViewProps {
  adapter: AutomationGraphAdapter;
  attachment: AutomationAttachment;
}

export function AutomationNotices({ state, noun }: { state: AutomationSnapshot; noun: string }) {
  return <>
    {state.loading && <p role="status" className="px-4 py-3 text-[var(--ct-muted-fg)]">Reading {noun}…</p>}
    {state.failed && !state.graph && <p role="status" className="px-4 py-3 text-[var(--ct-muted-fg)]">{noun} are unavailable.</p>}
    {state.graph?.truncated && <InlineErrorNotice error={{ code: "automation_incomplete", raw: "" }} />}
    {state.graph?.quarantined?.length ? (
      <ul role="status" aria-label="Quarantined Blueprints" className="px-4 py-3 text-[var(--ct-muted-fg)]">
        {state.graph.quarantined.map((entry) => (
          <li key={entry.id}>
            Quarantined {entry.name ?? entry.id}: {entry.code}: {entry.reason}
          </li>
        ))}
      </ul>
    ) : null}
  </>;
}

function AutomationGraphSurface({ adapter, attachment }: AutomationGraphViewProps) {
  const state = useAutomationGraph(adapter, attachment);
  const [scenarioId, setScenarioId] = useState<string | null>(null);
  const list = useRef<HTMLUListElement>(null);
  const removed = useRef<HTMLParagraphElement>(null);
  const returnTo = useRef<string | null>(null);
  const scenario = state.graph?.scenarios.find((entry) => entry.id === scenarioId);
  const missing = state.graph !== null && scenarioId !== null && !scenario;
  useLayoutEffect(() => {
    if (scenario) return;
    if (returnTo.current) focusEntry(list.current, "scenarioId", returnTo.current);
    else if (missing && focusLost()) removed.current?.focus();
    returnTo.current = null;
  }, [scenario, missing]);
  return <section aria-label="Automations" className="min-w-0 text-[length:var(--ct-font-size-sm)] text-[var(--ct-fg)]">
    {scenario && state.graph ? <>
      <AutomationNotices state={state} noun="automations" />
      <HostBlueprintView graph={state.graph} attachment={attachment} adapter={adapter} refresh={state.refresh} blueprintId={scenario.root.id} backLabel="All Blueprints" onBack={() => { returnTo.current = scenario.id; setScenarioId(null); }} />
    </> : <>
      <div className="ct-jobs-header px-4 py-3"><h2 className="text-[length:var(--ct-font-size-md)] font-semibold">Blueprints</h2></div>
      <AutomationNotices state={state} noun="automations" />
      {missing && <div ref={removed} tabIndex={-1}  className="px-4 py-3 outline-none"><InlineErrorNotice error={{ code: "blueprint_unavailable", raw: "" }} /></div>}
      {state.graph && <>
        {state.graph.scenarios.length === 0 && <p className="px-4 py-3 text-[var(--ct-muted-fg)]">No Blueprints recorded in this scope.</p>}
        <ul ref={list} aria-label="Blueprint scenarios" className="space-y-2 p-3">{blueprintScenarioJobs(state.graph, attachment).map((job) => {
          const Icon = Network;
          return <li key={job.scenario.id}><button type="button" data-scenario-id={job.scenario.id} className={AUTOMATION_ROW} onClick={() => setScenarioId(job.scenario.id)}>
            <Icon size={15} className="shrink-0 text-[var(--ct-muted-fg)]" aria-hidden="true" />
            <span className="min-w-0 flex-1 truncate font-medium">{blueprintTitle(job.scenario, job.root)}</span>
            <span className="shrink-0 text-xs text-[var(--ct-muted-fg)]"> · {job.type} · </span>
            <span className="flex shrink-0 items-center gap-1.5 text-xs text-[var(--ct-muted-fg)]"><span className={`h-1.5 w-1.5 rounded-full ${stateTone(job.state)}`} aria-hidden="true" />{job.state}</span>
            <ChevronRight size={14} className="shrink-0 text-[var(--ct-muted)]" aria-hidden="true" />
          </button></li>;
        })}</ul>
      </>}
    </>}
  </section>;
}

export function AutomationGraphView(props: AutomationGraphViewProps) {
  const { connection_id, workspace_id, team_id } = props.attachment;
  return <AutomationGraphSurface key={JSON.stringify([connection_id, workspace_id, team_id])} {...props} />;
}
