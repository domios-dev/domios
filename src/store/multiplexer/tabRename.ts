import { describeError } from "../../lib/describeError";
import type { Notice } from "../../lib/notices";
import { TitleLevel } from "../../lib/types";
import { uid } from "../../lib/uid";
import type { Action } from "./reducer";
import type { ExtendedState } from "./tree";

export interface TabRenamePorts {
  dispatch: (action: Action) => void;
  persist: (args: { workspaceId: string; tabId: string; title: string }) => Promise<unknown>;
  notify: (notice: Notice) => void;
}

export function tabRenameFailureNotice(workspaceId: string, tabId: string, error: unknown): Notice {
  const description = describeError({ code: "tab_rename_failed", raw: describeError(error).raw });
  return {
    kind: "toast",
    id: uid(),
    tabId,
    workspaceId,
    title: description.title,
    detail: JSON.stringify({ code: "tab_rename_failed", message: description.cause }),
    severity: "Error",
  };
}

/** Shows the user's title at once; a refused save restores the previous title and says why. */
export function commitTabRename(
  state: ExtendedState,
  ports: TabRenamePorts,
  workspaceId: string,
  tabId: string,
  title: string,
): Promise<void> {
  const tab = state.tabs[tabId];
  const workspace = state.workspaces.find((ws) => ws.id === workspaceId);
  if (!tab || !workspace?.tabs.includes(tabId)) return Promise.resolve();
  ports.dispatch({ type: "SET_TAB_TITLE", tabId, title, level: TitleLevel.User });
  return ports.persist({ workspaceId, tabId, title }).then(() => undefined, (error: unknown) => {
    ports.dispatch({ type: "REVERT_TAB_TITLE", tabId, attempted: title, title: tab.title, level: tab.title_level });
    ports.notify(tabRenameFailureNotice(workspaceId, tabId, error));
  });
}
