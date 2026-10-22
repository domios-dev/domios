import { useCallback, useMemo, useRef } from "react";
import { useMuxStateStore } from "../../store/multiplexer/selectorStore";
import { useSidebarPlansOptional } from "../Canvas/PlansProvider";
import { hostAutomationAdapter } from "../Automation/hostAdapter";
import type { AutomationGraph } from "../Automation/types";
import { createMentionRegistry, type MentionContext } from "../../lib/composerMentions";
import { tabMentionSource } from "../../lib/mentionSources/tabs";
import { taskMentionSource } from "../../lib/mentionSources/tasks";
import { planMentionSource } from "../../lib/mentionSources/plans";
import { blueprintMentionSource } from "../../lib/mentionSources/blueprints";
import { mentionScopeForTab } from "../../lib/mentionSources/scope";
import { watchMentionState } from "../../lib/mentionSources/live";

export function useComposerMentionSources(tabId: string) {
  const mux = useMuxStateStore();
  const { plans } = useSidebarPlansOptional();
  const cache = useRef(new Map<string, Promise<AutomationGraph>>());
  const lifetime = useRef(new AbortController());
  const scope = useCallback(() => mentionScopeForTab(mux.getState(), tabId), [mux, tabId]);
  const registry = useMemo(() => {
    const readGraph = ({ scope: caller }: MentionContext) => {
      const key = JSON.stringify([caller.workspaceId, caller.teamId]);
      let reading = cache.current.get(key);
      if (!reading) {
        const teamIds = caller.teamId ? [caller.teamId, null] : [null];
        reading = Promise.all(teamIds.map((teamId) => hostAutomationAdapter.read({ connection_id: "composer",
          workspace_id: caller.workspaceId!, team_id: teamId }, lifetime.current.signal))).then((graphs) => ({ ...graphs[0], nodes: graphs.flatMap((graph) => graph.nodes) }));
        cache.current.set(key, reading);
      }
      return reading;
    };
    return createMentionRegistry([
      tabMentionSource(mux.getState), taskMentionSource(() => plans), planMentionSource(() => plans),
      blueprintMentionSource(readGraph, () => mux.getState().tabs),
    ], {
      subscribe: (changed) => watchMentionState(mux, changed),
      close: () => { lifetime.current.abort(); lifetime.current = new AbortController(); cache.current.clear(); },
    });
  }, [mux, plans]);
  return { registry, scope };
}
