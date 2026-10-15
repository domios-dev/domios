import type { Workspace } from "../../lib/types";
import { invoke } from "../../lib/bridge";
import { describeError } from "../../lib/describeError";
import { recoverTabFolder } from "../../lib/tabFolderRecovery";
import { WorkspaceFolderError } from "../Sidebar/WorkspaceFolderError";
import { ErrorNotice, type ErrorNoticeAction } from "../shared/ErrorNotice";

export function TabLaunchFailure({ tabId, workspace, cwd, reason, actions = [] }: {
  tabId?: string;
  workspace?: Workspace;
  cwd?: string | null;
  reason: string | null;
  actions?: ErrorNoticeAction[];
}) {
  return workspace && describeError(reason).actions.includes("choose-folder")
    ? <WorkspaceFolderError workspace={workspace} rawError={reason} cwd={cwd}
      onRelocate={tabId ? path => recoverTabFolder(tabId, path, {
        update: (tabId, cwd) => invoke("mux_update_tab_cwd", { tabId, cwd }),
        restart: (tabId, fresh) => invoke("restart_agent_tab", { tabId, fresh }),
      }) : undefined} />
    : <ErrorNotice error={reason} operation="starting this tab" cwd={cwd} blocking actions={actions} />;
}
