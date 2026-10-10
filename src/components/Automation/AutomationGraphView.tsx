import { useLayoutEffect, useRef, useState } from "react";
import { ChevronRight, Network } from "lucide-react";
import { focusEntry, focusLost } from "./focus";
import { ScopedScenarioPanel } from "./ScopedScenarioPanel";
import { ScenarioPanel } from "./ScenarioPanel";
import { blueprintScenarioJobs, blueprintTitle } from "./scenarios";
import { AUTOMATION_ROW, stateTone } from "./styles";
import { ALL_JOBS_SCOPE, type AutomationAttachment, type AutomationGraphAdapter } from "./types";
import { useAutomationGraph } from "./useAutomationGraph";
import { createAutomationCamera, type AutomationCamera } from "./useAutomationViewport";
import type { AutomationSnapshot } from "./resource";

export interface AutomationGraphViewProps {
  adapter: AutomationGraphAdapter;
  attachment: AutomationAttachment;
}

export function AutomationNotices({ state, noun }: { state: AutomationSnapshot; noun: string }) {
  return <>
    {state.loading && <p role="status" className="px-4 py-3 text-[var(--ct-muted-fg)]">Reading {noun}…</p>}
    {state.failed && <p role="alert" className="px-4 py-3">Automations unavailable. Retained records are stale; stopping is disabled.</p>}
    {state.graph?.truncated && <p role="alert" className="px-4 py-3">This automation snapshot is incomplete. Some participants or links are unavailable.</p>}
  </>;
}

const cameras = new Map<string, AutomationCamera>();

/** Cameras outlive the view, so a reopened Blueprint keeps its framing and selected node. */
export function scenarioCamera(scenarioId: string): AutomationCamera {
  const existing = cameras.get(scenarioId);
  if (existing) return existing;
  const created = createAutomationCamera();
  cameras.set(scenarioId, created);
  return created;
}

export function useScenarioCameras() {
  return scenarioCamera;
}

function AutomationGraphSurface({ adapter, attachment }: AutomationGraphViewProps) {
  const state = useAutomationGraph(adapter, attachment);
  const [scenarioId, setScenarioId] = useState<string | null>(null);
  const cameraFor = useScenarioCameras();
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
    {scenario && attachment.team_id === ALL_JOBS_SCOPE ? <ScopedScenarioPanel adapter={adapter} attachment={attachment} scenario={scenario} camera={cameraFor(scenario.id)} backLabel="All Blueprints" onBack={() => { returnTo.current = scenario.id; setScenarioId(null); }} /> : scenario ? <ScenarioPanel state={state} scope={attachment} scenario={scenario} camera={cameraFor(scenario.id)} backLabel="All Blueprints" onBack={() => { returnTo.current = scenario.id; setScenarioId(null); }}>
      <AutomationNotices state={state} noun="automations" />
    </ScenarioPanel> : <>
      <div className="ct-jobs-header px-4 py-3"><h2 className="text-[length:var(--ct-font-size-md)] font-semibold">Blueprints</h2></div>
      <AutomationNotices state={state} noun="automations" />
      {missing && <p ref={removed} tabIndex={-1} role="alert" className="px-4 py-3 outline-none">The selected Blueprint is unavailable. Select a recorded Blueprint to continue.</p>}
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
