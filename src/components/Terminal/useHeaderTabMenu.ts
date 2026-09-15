import { useCallback } from "react";
import { anchorsTeam } from "../../lib/roleKind";
import type { ProviderDescriptor, Tab } from "../../lib/types";
import { useAgentDescriptors } from "../../lib/useAgentDescriptor";
import { useMultiplexer, useMuxSelector } from "../../store/multiplexer";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { usePlanIndexOptional } from "../Canvas/PlansProvider";
import { resolveTabTabActivity, type TabTabActivity } from "../Canvas/tabTabActivity";
import type { HeaderTabMenu } from "./TabTopBar";

function selectOrchestrators(state: ExtendedState): Tab[] {
  return Object.values(state.tabs).filter((tab) => anchorsTeam(tab));
}

function sameTabs(a: Tab[], b: Tab[]): boolean {
  return a.length === b.length && a.every((tab, index) => tab === b[index]);
}

function sameActivity(a: TabTabActivity | null, b: TabTabActivity | null): boolean {
  if (a === null || b === null) return a === b;
  return a.kind === b.kind && a.planId === b.planId;
}

export function useHeaderTabMenu(
  tab: Tab | undefined,
  workspaceId: string | null,
  descriptor: ProviderDescriptor | undefined,
): HeaderTabMenu | null {
  const tabId = tab?.id ?? null;
  const { closeTab, setTabTitle } = useMultiplexer();
  const { index } = usePlanIndexOptional();
  const descriptors = useAgentDescriptors();
  const status = useMuxSelector(useCallback((state: ExtendedState) => (tabId ? state.tabStatuses[tabId] ?? null : null), [tabId]));
  const orchestrators = useMuxSelector(selectOrchestrators, sameTabs);
  const activity = useMuxSelector(
    useCallback((state: ExtendedState) => resolveTabTabActivity(index, tabId, state.tabs), [index, tabId]),
    sameActivity,
  );
  if (!tab) return null;
  const id = tab.id;
  const ws = workspaceId;
  return {
    tab,
    context: { status, descriptor, descriptors, activity, orchestrators },
    onRename: ws ? (title) => setTabTitle(ws, id, title) : undefined,
    onClose: ws ? () => closeTab(ws, id) : undefined,
  };
}
