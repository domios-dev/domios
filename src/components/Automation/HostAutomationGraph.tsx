import { usePlanPeerContext } from "../Canvas/usePlanPeerContext";
import { AutomationGraphView } from "./AutomationGraphView";
import { hostAutomationAdapter } from "./hostAdapter";
import type { AutomationGraphAdapter, AutomationScope } from "./types";

export function HostAutomationGraph({ workspace_id, team_id, adapter = hostAutomationAdapter }: AutomationScope & { adapter?: AutomationGraphAdapter }) {
  const context = usePlanPeerContext();
  if (!context.ready) return <p role="status">Attaching automations…</p>;
  return <AutomationGraphView adapter={adapter} attachment={{ workspace_id, team_id, connection_id: `host:${context.generation}` }} />;
}
