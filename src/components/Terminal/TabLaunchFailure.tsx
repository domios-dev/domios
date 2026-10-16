import { useContext } from "react";
import type { Workspace } from "../../lib/types";
import { invoke } from "../../lib/bridge";
import { describeError } from "../../lib/describeError";
import { recoverTabFolder } from "../../lib/tabFolderRecovery";
import { MultiplexerActionsContext } from "../../store/multiplexer/context";
import { WorkspaceFolderError } from "../Sidebar/WorkspaceFolderError";
import type { ErrorNoticeAction } from "../shared/ErrorNotice";
import { LaunchFailureEmptyState } from "./LaunchFailureEmptyState";

export function TabLaunchFailure({ tabId, workspace, cwd, reason, actions = [] }: {
  tabId?: string;
  workspace?: Workspace;
  cwd?: string | null;
  reason: string | null;
  actions?: ErrorNoticeAction[];
}) {
  const mux = useContext(MultiplexerActionsContext);
  const missingFolder = describeError(reason).actions.includes("choose-folder");
  const close = tabId && workspace && mux ? () => mux.closeTab(workspace.id, tabId) : undefined;
  const notice = (chooseFolder?: () => void, busy = false) => <LaunchFailureEmptyState
    reason={reason} path={cwd ?? workspace?.cwd} missingFolder={missingFolder}
    actions={actions} onChooseFolder={chooseFolder} busy={busy} onClose={close} />;
  return workspace
    ? <WorkspaceFolderError workspace={workspace} rawError={reason} cwd={cwd}
      renderNotice={(chooseFolder, busy) => notice(chooseFolder, busy)}
      onRelocate={tabId ? path => recoverTabFolder(tabId, path, {
        update: (tabId, cwd) => invoke("mux_update_tab_cwd", { tabId, cwd }),
        restart: (tabId, fresh) => invoke("restart_agent_tab", { tabId, fresh }),
      }) : undefined} />
    : notice();
}
