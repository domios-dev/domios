import { useContext } from "react";
import type { Workspace } from "../../lib/types";
import { invoke } from "../../lib/bridge";
import { recoverTabFolder } from "../../lib/tabFolderRecovery";
import { MultiplexerActionsContext } from "../../store/multiplexer/context";
import { FolderPaneState } from "../shared/FolderPaneState";
import type { ErrorNoticeAction } from "../shared/ErrorNotice";
import { paneStateModel } from "../shared/paneStateModel";
import { PaneState } from "../shared/PaneState";

export function TabLaunchFailure({ tabId, workspace, cwd, reason, actions = [] }: {
  tabId?: string;
  workspace?: Workspace;
  cwd?: string | null;
  reason: string | null;
  actions?: ErrorNoticeAction[];
}) {
  const mux = useContext(MultiplexerActionsContext);
  const close = tabId && workspace && mux ? () => mux.closeTab(workspace.id, tabId) : undefined;
  return workspace && paneStateModel(reason).actions.includes("choose-folder")
    ? <FolderPaneState workspace={workspace} error={reason} cwd={cwd} actions={actions} onClose={close}
      onRelocate={tabId ? path => recoverTabFolder(tabId, path, {
        update: (tabId, cwd) => invoke("mux_update_tab_cwd", { tabId, cwd }),
        restart: (tabId, fresh) => invoke("restart_agent_tab", { tabId, fresh }),
      }) : undefined} />
    : <PaneState error={reason} context={{ scope: "tab", operation: "starting this tab", cwd }} actions={actions} secondaryActions={close ? [{ label: "Close tab", onClick: close }] : []} />;
}
