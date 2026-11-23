import { AgentStateIndicator } from "../../../../src/components/Terminal/AgentStateIndicator";
import { TabStatusLineModelPicker } from "../../../../src/components/Terminal/StatusLineModelPicker";
import { getAgentDescriptor } from "../../../../src/lib/agentDescriptorCache";
import { tabActivity } from "../../../../src/lib/tabActivity";
import type { Tab } from "../../../../src/lib/types";
import { ProductProviders } from "../../gallery/fixtures";
import { MEMBER_STATUS } from "./team";

const noop = () => undefined;

/** The chat status row exactly as TabSessionChat mounts it, for one Team member. */
export function StatusRow({ tab }: { tab: Tab }) {
  const provider = getAgentDescriptor(tab.agent_type);
  return <ProductProviders tabs={[tab]}>
    <div className="ct-composer-status w-full">
      <AgentStateIndicator
        activity={tabActivity(tab, MEMBER_STATUS[tab.id])}
        providerName={provider?.display_name}
        model={tab.agent_session_model}
        reasoningEffort={{ actual: "high", actual_source: "session-owned" }}
        className="ct-composer-lane mx-auto w-full"
        onStop={noop}
        onCancel={noop}
        onOpenTerminal={noop}
        renderIdentity={(label) => <TabStatusLineModelPicker tab={tab} label={label} />}
      />
    </div>
  </ProductProviders>;
}
