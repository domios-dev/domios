import { useMemo } from "react";
import { useMuxSelector } from "../../store/multiplexer/selectorStore";
import { useSidebarPlansOptional } from "../Canvas/PlansProvider";
import { hostAutomationAdapter } from "../Automation/hostAdapter";
import { ALL_JOBS_SCOPE } from "../Automation/types";
import { createMentionRegistry, type MentionScope } from "../../lib/composerMentions";
import { tabMentionSource } from "../../lib/mentionSources/tabs";
import { taskMentionSource } from "../../lib/mentionSources/tasks";
import { planMentionSource } from "../../lib/mentionSources/plans";
import { blueprintMentionSource } from "../../lib/mentionSources/blueprints";

export function useComposerMentionSources(tabId: string) {
  const tabs = useMuxSelector((state) => state.tabs);
  const workspaces = useMuxSelector((state) => state.workspaces);
  const { plans } = useSidebarPlansOptional();
  const tab = tabs[tabId];
  const workspace = workspaces.find((item) => item.id === tab?.workspace_id || item.tabs.includes(tabId));
  const scope = useMemo<MentionScope>(() => ({ tabId, workspaceId: workspace?.id ?? null,
    projectPath: workspace?.cwd ?? null, teamId: tab?.team_id ?? null }), [tabId, workspace?.id, workspace?.cwd, tab?.team_id]);
  const registry = useMemo(() => createMentionRegistry([
    tabMentionSource(() => ({ tabs, workspaces })), taskMentionSource(() => plans), planMentionSource(() => plans),
    blueprintMentionSource(({ scope: caller, signal }) => hostAutomationAdapter.read({ connection_id: "composer",
      workspace_id: caller.workspaceId!, team_id: ALL_JOBS_SCOPE }, signal), () => tabs),
  ]), [tabs, workspaces, plans]);
  return { registry, scope };
}
