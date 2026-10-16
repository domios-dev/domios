import type { ReactNode } from "react";
import type { Workspace } from "../../lib/types";
import type { ErrorNoticeAction } from "./ErrorNotice";
import { InlineErrorNotice } from "./ErrorNotice";
import { PaneState } from "./PaneState";
import { paneStateModel } from "./paneStateModel";
import { RemoteFolderPicker } from "./RemoteFolderPicker";
import { useFolderRecovery } from "./useFolderRecovery";

export function FolderPaneState({ workspace, error, cwd, onRelocate, onClose, actions = [] }: {
  workspace: Workspace;
  error?: unknown;
  cwd?: string | null;
  onRelocate?: (path: string) => Promise<unknown>;
  onClose?: () => void;
  actions?: ErrorNoticeAction[];
}) {
  const recovery = useFolderRecovery(workspace, onRelocate);
  const scope = onRelocate ? "tab" : "workspace";
  const context = { scope, cwd: cwd ?? workspace.cwd, operation: "starting this tab" } as const;
  const fault = error ?? { code: "workspace_cwd_missing", raw: `Workspace folder unavailable: ${workspace.cwd}` };
  const folderAction = paneStateModel(fault, context).actions.includes("choose-folder");
  const primary = folderAction ? [{ label: "Choose folder", disabled: recovery.busy, onClick: () => { void recovery.chooseFolder(); } }] : actions;
  return <>
    <PaneState error={fault} context={context} actions={primary} secondaryActions={onClose ? [{ label: "Close tab", onClick: onClose }] : []} detailActions={folderAction ? actions : []} />
    {recovery.error != null && <InlineErrorNotice error={recovery.error} operation="choosing a workspace folder" blocking />}
    {recovery.remotePicker && <RemoteFolderPicker title={`Choose folder for ${workspace.name}`} onCancel={recovery.cancel} onSelect={path => { void recovery.relocate(path); }} />}
  </>;
}

export function WorkspacePaneState({ workspace, visible = true, children }: { workspace: Workspace; visible?: boolean; children?: ReactNode }) {
  if (!workspace.cwd_missing) return children;
  if (!visible) return null;
  return <div className="absolute inset-0 z-30"><FolderPaneState workspace={workspace} /></div>;
}
