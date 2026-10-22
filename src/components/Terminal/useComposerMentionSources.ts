import { useCallback, useEffect, useMemo, useRef } from "react";
import { useMuxStateStore } from "../../store/multiplexer/selectorStore";
import { useSidebarPlansOptional, usePlansApiOptional } from "../Canvas/PlansProvider";
import { daemonFetch } from "../../lib/webApi";
import { fileMentionSource, type MentionFile } from "../../lib/mentionSources/files";
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
  const api = usePlansApiOptional();
  const planListeners = useRef(new Set<() => void>());
  useEffect(() => { for (const changed of planListeners.current) changed(); }, [plans]);
  const plansRef = useRef(plans);
  plansRef.current = plans;
  const cache = useRef(new Map<string, Promise<AutomationGraph>>());
  const lifetime = useRef(new AbortController());
  const scope = useCallback(() => mentionScopeForTab(mux.getState(), tabId), [mux, tabId]);
  const registry = useMemo(() => {
    const files = new Map<string, Promise<readonly MentionFile[]>>();
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
      tabMentionSource(mux.getState), taskMentionSource(() => plansRef.current), planMentionSource(() => plansRef.current),
      blueprintMentionSource(readGraph, () => mux.getState().tabs),
      fileMentionSource(async (path, signal) => {
        if (!api) return [];
        signal.throwIfAborted();
        let reading = files.get(path);
        if (!reading) {
          reading = daemonFetch(api, `/api/fs/list-dir?path=${encodeURIComponent(path)}&files_only=true`, { signal: lifetime.current.signal }).then(async response => {
            if (!response.ok) throw new Error(`File suggestions unavailable: ${response.status}`);
            return (await response.json() as { entries: MentionFile[] }).entries;
          }).catch(error => { files.delete(path); throw error; });
          files.set(path, reading);
        }
        return reading;
      }),
    ], {
      subscribe: (changed) => {
        planListeners.current.add(changed);
        const offMux = watchMentionState(mux, changed);
        return () => { planListeners.current.delete(changed); offMux(); };
      },
      close: () => { lifetime.current.abort(); lifetime.current = new AbortController(); cache.current.clear(); files.clear(); },
    });
  }, [mux, api]);
  return { registry, scope };
}
