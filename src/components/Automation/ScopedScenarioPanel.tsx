import { ScenarioPanel } from "./ScenarioPanel";
import { useAutomationGraph } from "./useAutomationGraph";
import { BackPill } from "../shared/BackPill";
import type { AutomationAttachment, AutomationGraphAdapter, AutomationScenario } from "./types";
import type { AutomationCamera } from "./useAutomationViewport";

export function ScopedScenarioPanel({ adapter, attachment, scenario, camera, backLabel, onBack, headingLevel, fill, initialRunId }: {
  adapter: AutomationGraphAdapter; attachment: AutomationAttachment; scenario: AutomationScenario;
  camera: AutomationCamera; backLabel: string; onBack: () => void; headingLevel?: 2 | 3; fill?: boolean; initialRunId?: string;
}) {
  const scope = { ...attachment, team_id: scenario.root.team_id };
  const state = useAutomationGraph(adapter, scope);
  if (!state.graph) return <div className="flex flex-col items-start gap-3 p-4"><BackPill label={backLabel} onBack={onBack} /><p role={state.failed ? "alert" : "status"} className="ct-job-note">{state.failed ? "Blueprint unavailable." : "Reading Blueprint…"}</p></div>;
  return <ScenarioPanel key={JSON.stringify([attachment.connection_id, attachment.workspace_id, scenario.id, scenario.root.team_id])} state={state} scope={scope} scenario={scenario} camera={camera} backLabel={backLabel} onBack={onBack} headingLevel={headingLevel} fill={fill} initialRunId={initialRunId}>
    {state.failed && <p role="alert" className="ct-job-note px-4 py-2">Blueprint unavailable. Retained records are stale.</p>}
  </ScenarioPanel>;
}
