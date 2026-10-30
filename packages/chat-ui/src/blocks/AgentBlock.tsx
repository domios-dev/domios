import { Bot } from "lucide-react";
import { ChatBlockFrame, type ChatBlockAction } from "./ChatBlockFrame";
import { useChatBlockHost, type AgentBlockHost } from "./ChatBlockHost";
import { useLiveBinding } from "./liveBinding";
import { BlockNotice } from "./BlockNotice";
import {
  AGENT_STATE_LABEL,
  AGENT_STATE_TONE,
  agentActivity,
  agentIdentity,
  type AgentBlockInfo,
  type AgentBlockState,
} from "./agentBlockModel";

const LOADING: AgentBlockState = { kind: "loading" };

/** `@ds-agent <tab-id>`: a live agent card; Open and a confirmed Stop run only from the user's click. */
export function AgentBlock({ tabRef, source }: { tabRef: string; source: string }) {
  const host = useChatBlockHost().agents;
  const state = useLiveBinding(host?.binding, tabRef, LOADING);
  if (!host) return <BlockNotice name="agent" source={source} label={`Agent ${tabRef}`} text="Agent cards are unavailable in this view." icon={Bot} />;
  switch (state.kind) {
    case "loading":
      return <BlockNotice name="agent" source={source} label={`Agent ${tabRef}`} text="Looking for this agent…" icon={Bot} />;
    case "missing":
      return <BlockNotice name="agent" source={source} label={`Agent ${tabRef}`} text={`No open tab matches “${tabRef}”. It may have closed.`} icon={Bot} missing />;
    case "ready":
      return <AgentCard agent={state.agent} host={host} tabRef={tabRef} source={source} />;
  }
}

function AgentCard({ agent, host, tabRef, source }: { agent: AgentBlockInfo; host: AgentBlockHost; tabRef: string; source: string }) {
  const activity = agentActivity(agent);
  const identity = agentIdentity(agent);
  const actions: ChatBlockAction[] = [{ id: "open", label: "Open", primary: true, run: () => host.open(tabRef) }];
  if (agent.canStop) {
    actions.push({
      id: "stop",
      label: "Stop",
      busyLabel: "Stopping…",
      confirm: { message: `Stop ${agent.name}'s current turn? The tab stays open.`, confirmLabel: "Stop turn" },
      run: async () => {
        await host.stop(tabRef);
        return `Asked ${agent.name} to stop.`;
      },
    });
  }
  return (
    <ChatBlockFrame
      name="agent"
      source={source}
      title={agent.name}
      icon={<Bot size={14} aria-hidden="true" />}
      status={{ label: AGENT_STATE_LABEL[agent.state], tone: AGENT_STATE_TONE[agent.state] }}
      summary={identity || undefined}
      body={activity ? (
        <p data-testid="agent-activity" className="m-0 truncate text-xs" style={{ color: "var(--ct-fg)" }} title={activity}>
          {agent.task && <span className="mr-1.5 tabular-nums" style={{ color: "var(--ct-muted-fg)" }}>{agent.task.key}</span>}
          {agent.task ? agent.task.name : activity}
        </p>
      ) : undefined}
      details={(
        <dl className="m-0 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
          {agent.role && <><dt>Role</dt><dd className="m-0">{agent.role}</dd></>}
          {agent.provider && <><dt>Provider</dt><dd className="m-0">{agent.provider}</dd></>}
          {agent.model && <><dt>Model</dt><dd className="m-0 break-all">{agent.model}</dd></>}
          {agent.task && <><dt>Plan</dt><dd className="m-0">{agent.task.plan}</dd></>}
          <dt>Tab</dt><dd className="m-0 font-mono">{agent.tabId}</dd>
        </dl>
      )}
      actions={actions}
      askText={`About the agent ${agent.name} (\`${source}\`): `}
      muted={agent.state === "offline"}
    />
  );
}
