import type { AgentBlockHost, AgentBlockState } from "@codeterm/chat-ui";
import type { Plan } from "../../components/Canvas/types";
import { interruptAgentArgs } from "../agentInterrupt";
import type { MultiplexerState, Tab, TabStatus } from "../types";
import { agentBlockState, resolveTabRef } from "./agentBlockState";
import type { ValueFeed } from "./valueFeed";

export interface AgentBlockSources {
  mux: { getState(): Pick<MultiplexerState, "tabs" | "workspaces"> & { tabStatuses?: Record<string, TabStatus> }; subscribe(listener: () => void): () => void };
  plans: ValueFeed<readonly Plan[]>;
  providerName(agentType: string): string | null;
  selectTab(workspaceId: string, tabId: string): Promise<void> | void;
  interrupt(args: { ptyId?: string; tabId: string }): Promise<unknown>;
}

/** Agent cards read the mux snapshot and plans; Open focuses the tab and Stop interrupts its turn. */
export function createAgentBlockHost(sources: AgentBlockSources): AgentBlockHost {
  let cachedTabs: unknown = null;
  let cachedPlans: unknown = null;
  let cachedStatuses: unknown = null;
  const states = new Map<string, AgentBlockState>();

  const tab = (ref: string): Tab => {
    const found = resolveTabRef(sources.mux.getState().tabs, ref);
    if (!found) throw new Error("this tab is closed");
    return found;
  };

  return {
    binding: {
      subscribe(listener) {
        const offMux = sources.mux.subscribe(listener);
        const offPlans = sources.plans.subscribe(listener);
        return () => {
          offMux();
          offPlans();
        };
      },
      read(ref) {
        const { tabs, tabStatuses } = sources.mux.getState();
        const plans = sources.plans.get();
        if (tabs !== cachedTabs || plans !== cachedPlans || tabStatuses !== cachedStatuses) {
          cachedTabs = tabs;
          cachedStatuses = tabStatuses;
          cachedPlans = plans;
          states.clear();
        }
        let state = states.get(ref);
        if (!state) {
          state = agentBlockState(tabs, plans, ref, sources.providerName, tabStatuses);
          states.set(ref, state);
        }
        return state;
      },
      refresh: () => {},
    },
    open(ref) {
      const target = resolveTabRef(sources.mux.getState().tabs, ref);
      if (!target) return;
      const workspace = sources.mux.getState().workspaces.find((candidate) => candidate.tabs.includes(target.id));
      const workspaceId = workspace?.id ?? target.workspace_id;
      if (workspaceId) void sources.selectTab(workspaceId, target.id);
    },
    async stop(ref) {
      const args = interruptAgentArgs(tab(ref));
      if (!args) throw new Error("this agent has no running process");
      await sources.interrupt(args);
    },
  };
}
