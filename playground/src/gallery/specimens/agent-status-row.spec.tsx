import { AgentStateIndicator } from "../../../../src/components/Terminal/AgentStateIndicator";
import { TabStatusLineModelPicker } from "../../../../src/components/Terminal/StatusLineModelPicker";
import { getAgentDescriptor } from "../../../../src/lib/agentDescriptorCache";
import { tabActivity } from "../../../../src/lib/tabActivity";
import type { Tab, TabStatus } from "../../../../src/lib/types";
import { agentTab, CODEX, ProductProviders } from "../fixtures";
import { PinnedMenu } from "../stageTools";
import { defineSpecimen } from "../types";

const noop = () => undefined;
const effort = { actual: "high", actual_source: "session-owned" as const };

type StatusRowProps = { tab: Tab; status?: TabStatus; rebind?: boolean; picker?: boolean };

function StatusRowSpecimen({ tab, status, rebind = false, picker = false }: StatusRowProps) {
  const provider = getAgentDescriptor(tab.agent_type);
  const row = <div className="ct-composer-status w-full">
    <AgentStateIndicator
      activity={tabActivity(tab, status)}
      providerName={provider?.display_name}
      model={tab.agent_session_model}
      reasoningEffort={tab.reasoning_effort_identity}
      className="ct-composer-lane mx-auto w-full"
      onStop={noop}
      onCancel={noop}
      onOpenTerminal={noop}
      onRebind={rebind ? noop : undefined}
      renderIdentity={(label) => <TabStatusLineModelPicker tab={tab} label={label} />}
    />
  </div>;
  return <ProductProviders tabs={[tab]}>
    <div className="gallery-fill">
      {picker ? <PinnedMenu trigger='[data-testid="status-model-trigger"]' via="click" menuSelector=".ct-model-picker">{row}</PinnedMenu> : row}
    </div>
  </ProductProviders>;
}

const tab = (id: string, overrides: Partial<Tab> = {}) => agentTab(id, { reasoning_effort_identity: effort, ...overrides });

export default defineSpecimen({
  id: "agent-status-row",
  title: "Agent status row",
  stage: "wide",
  category: "Status",
  where: "src/components/Terminal/AgentStateIndicator.tsx · StatusLineModelPicker.tsx",
  notes: "The chat status row exactly as TabSessionChat mounts it: the switchable model capsule, the activity dots, the provider status line and the Stop pill. At rest the dots stay in place, grey and still, and the Stop slot keeps its width.",
  Component: StatusRowSpecimen,
  states: [
    { id: "working", label: "Working", props: { tab: tab("status-working", { agent_state: { state: "working", status_line: "Planning gallery catalog… (9m 23s · ↓ 35.3k tokens)" } }) } },
    { id: "thinking", label: "Thinking", props: { tab: tab("status-thinking", { agent_state: { state: "working", phase: "thinking", status_line: "Swooping… (6m 24s · still thinking with high effort)" } }) } },
    { id: "streaming", label: "Streaming", props: { tab: tab("status-streaming", { agent_state: { state: "working", phase: "streaming", status_line: "Composing the answer… (18s · ↑ 2.1k tokens)" } }) } },
    { id: "idle", label: "Idle · dots at rest", props: { tab: tab("status-idle", { agent_state: { state: "idle", status_line: "Churned for 1m 37s" } }) } },
    { id: "question", label: "Waiting on you · permission", props: { tab: tab("status-question", { agent_state: { state: "question", subtype: "permission" } }) } },
    { id: "quiet", label: "Quiet · repair binding", props: { tab: tab("status-quiet", { agent_state: { state: "quiet" as never } }), rebind: true } },
    { id: "codex", label: "Codex identity", props: { tab: tab("status-codex", { agent_type: CODEX.id, agent_session_model: CODEX.available_models[0]?.id, agent_state: { state: "working", status_line: "Working (1m 02s)" } }) } },
    { id: "static", label: "Not switchable · protected", props: { tab: tab("status-static", { protected: true, agent_state: { state: "working", status_line: "Working" } }) } },
    { id: "picker", label: "Model picker open", props: { tab: tab("status-picker"), picker: true } },
  ],
});
