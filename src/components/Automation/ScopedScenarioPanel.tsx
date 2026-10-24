import { useState } from "react";
import { BlueprintPipelineCanvas } from "./BlueprintPipelineCanvas";
import { scenarioPipelineModel } from "./scenarioPipelineModel";
import { ScenarioPanel } from "./ScenarioPanel";
import { useAutomationGraph } from "./useAutomationGraph";
import { BackPill } from "../shared/BackPill";
import { BlueprintExportButton } from "./BlueprintExportButton";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { automationKey } from "./types";
import type { AutomationAttachment, AutomationGraphAdapter, AutomationScenario } from "./types";
import type { AutomationCamera } from "./useAutomationViewport";

export function ScopedScenarioPanel({ adapter, attachment, scenario, camera, backLabel, onBack, headingLevel, fill, initialRunId }: {
  adapter: AutomationGraphAdapter; attachment: AutomationAttachment; scenario: AutomationScenario;
  camera: AutomationCamera; backLabel: string; onBack: () => void; headingLevel?: 2 | 3; fill?: boolean; initialRunId?: string;
}) {
  const [showNative, setShowNative] = useState(false);
  const scope = { ...attachment, team_id: scenario.root.team_id };
  const state = useAutomationGraph(adapter, scope);
  if (!state.graph) return <div className="flex flex-col items-start gap-3 p-4"><BackPill label={backLabel} onBack={onBack} /><p role="status" className="ct-job-note">{state.failed ? "Blueprint unavailable." : "Reading Blueprint…"}</p></div>;
  const native = state.graph.blueprints?.find((blueprint) => blueprint.id === scenario.root.id && blueprint.call_graph);
  const root = state.graph.nodes.find((node) => automationKey(node.identity) === automationKey(scenario.root));
  if (showNative && native) return <div className="min-w-0 p-4"><div className="flex flex-wrap items-center gap-3"><BackPill label={backLabel} onBack={onBack} /><h3>{native.name}</h3><BlueprintExportButton workspaceId={scope.workspace_id} teamId={scope.team_id} id={scenario.root.id} /><button type="button" disabled={!root?.stop.allowed || state.failed || state.stopping !== null} onClick={() => void state.stop(scenario.root)}>Stop Blueprint</button><button type="button" onClick={() => setShowNative(false)}>Run view</button></div>{state.stopFailed && <InlineErrorNotice error={{ code: "blueprint_stop_failed", raw: "" }} />}<BlueprintPipelineCanvas pipeline={scenarioPipelineModel(state.graph, scenario, native)} initialRunId={initialRunId} /></div>;
  return <>{native && <button type="button" onClick={() => setShowNative(true)}>Typed graph</button>}<ScenarioPanel key={JSON.stringify([attachment.connection_id, attachment.workspace_id, scenario.id, scenario.root.team_id])} state={state} scope={scope} scenario={scenario} camera={camera} backLabel={backLabel} onBack={onBack} headingLevel={headingLevel} fill={fill} initialRunId={initialRunId} /></>;
}
