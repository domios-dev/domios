import { useLatestCallback } from "../../hooks/useLatestCallback";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import type { ExtendedState } from "../../store/multiplexer/tree";

interface ChatWorkspace {
  workspaceId: string | null;
  cwd?: string | null;
}

function sameChatWorkspace(left: ChatWorkspace, right: ChatWorkspace): boolean {
  return left.workspaceId === right.workspaceId && left.cwd === right.cwd;
}

export function useChatWorkspace(tabId: string): ChatWorkspace {
  const selectWorkspace = useLatestCallback((state: ExtendedState): ChatWorkspace => {
    const tab = state.tabs[tabId];
    const workspace = state.workspaces.find((item) => item.tabs.includes(tabId))
      ?? state.workspaces.find((item) => item.id === tab?.workspace_id);
    return {
      workspaceId: workspace?.id ?? tab?.workspace_id ?? null,
      cwd: tab?.cwd ?? workspace?.cwd,
    };
  }, [tabId]);
  return useMuxSelector(selectWorkspace, sameChatWorkspace);
}
