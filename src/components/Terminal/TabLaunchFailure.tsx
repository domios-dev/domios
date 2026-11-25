import { useContext } from "react";
import type { Workspace } from "../../lib/types";
import { invoke } from "../../lib/bridge";
import { recoverTabFolder } from "../../lib/tabFolderRecovery";
import { MultiplexerActionsContext } from "../../store/multiplexer/context";
import { FolderPaneState } from "../shared/FolderPaneState";
import { InlineErrorNotice, type ErrorNoticeAction } from "../shared/ErrorNotice";
import type { ErrorDescription } from "../../lib/describeError";
import { paneStateModel } from "../shared/paneStateModel";
import { PaneState } from "../shared/PaneState";
import { parseProviderNotInstalled } from "../../lib/providerNotInstalled";
import { ProviderNotInstalledNotice } from "./ProviderNotInstalledNotice";

export function relocateTabFolder(tabId: string, path: string) {
  return recoverTabFolder(tabId, path, {
    update: (tabId, cwd) => invoke("mux_update_tab_cwd", { tabId, cwd }),
    restart: (tabId, fresh) => invoke("restart_agent_tab", { tabId, fresh }),
  });
}

export function TabLaunchFailure({ tabId, workspace, cwd, reason, actions = [], description, inline = false }: {
  tabId?: string;
  workspace?: Workspace;
  cwd?: string | null;
  reason: string | null;
  actions?: ErrorNoticeAction[];
  description?: ErrorDescription;
  inline?: boolean;
}) {
  const mux = useContext(MultiplexerActionsContext);
  const close = tabId && workspace && mux ? () => mux.closeTab(workspace.id, tabId) : undefined;
  const notInstalled = parseProviderNotInstalled(reason);
  if (notInstalled) {
    return <ProviderNotInstalledNotice
      info={notInstalled}
      rawReason={reason ?? ""}
      inline={inline}
      onRetry={actions[0]?.onClick}
      onClose={close}
    />;
  }
  if (inline) return <InlineErrorNotice error={reason} description={description} actions={actions} operation="restoring this session" />;
  return workspace && paneStateModel(reason).actions.includes("choose-folder")
    ? <FolderPaneState workspace={workspace} error={reason} cwd={cwd} actions={actions} onClose={close}
      onRelocate={tabId ? path => relocateTabFolder(tabId, path) : undefined} />
    : <PaneState error={reason} context={{ scope: "tab", operation: "starting this tab", cwd, description }} actions={actions} secondaryActions={close ? [{ label: "Close tab", onClick: close }] : []} />;
}
