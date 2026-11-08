import type { Tab, Workspace } from "../../lib/types";
import { isReadOnlyTab } from "../../lib/tabInputPolicy";
import { tabFailureDescription, type TabFailure } from "../../lib/tabFailure";
import { useMuxSelector } from "../../store/multiplexer/selectorStore";
import { ErrorNotice, InlineErrorNotice, type ErrorNoticeAction } from "../shared/ErrorNotice";
import { paneStateModel } from "../shared/paneStateModel";
import { RemoteFolderPicker } from "../shared/RemoteFolderPicker";
import { useFolderRecovery } from "../shared/useFolderRecovery";
import { relocateTabFolder } from "./TabLaunchFailure";
import { useTabRecovery } from "./useTabRecovery";

/** A launched session's failure in the Session status row: its truthful name and its recovery, beside the transcript. */
export function TabFailureNotice({ tab, failure, detail, workspaceId }: {
  tab: Tab;
  failure: TabFailure;
  detail: string | null;
  workspaceId: string | null;
}) {
  const { actions, error } = useTabRecovery(tab.id, isReadOnlyTab(tab) ? null : failure.recovery);
  const workspace = useMuxSelector(state => state.workspaces.find(candidate => candidate.id === workspaceId));
  const missingFolder = !isReadOnlyTab(tab) && paneStateModel(detail).actions.includes("choose-folder");
  return <div className="w-full">
    {missingFolder && workspace
      ? <FolderRecoveryNotice tab={tab} workspace={workspace} detail={detail} failure={failure} actions={actions} />
      : <ErrorNotice error={detail} description={tabFailureDescription(failure, detail) ?? undefined} actions={actions} operation="resuming this session" />}
    {error && <InlineErrorNotice error={error} operation="recovering this session" />}
  </div>;
}

function FolderRecoveryNotice({ tab, workspace, detail, failure, actions }: {
  tab: Tab;
  workspace: Workspace;
  detail: string | null;
  failure: TabFailure;
  actions: ErrorNoticeAction[];
}) {
  const recovery = useFolderRecovery(workspace, path => relocateTabFolder(tab.id, path));
  const choose = { label: "Choose folder", disabled: recovery.busy, onClick: () => { void recovery.chooseFolder(); } };
  return <>
    <ErrorNotice error={detail} description={tabFailureDescription(failure, detail) ?? undefined} actions={[choose, ...actions]} operation="resuming this session" />
    {recovery.error != null && <InlineErrorNotice error={recovery.error} operation="choosing a folder" />}
    {recovery.remotePicker && <RemoteFolderPicker title={`Choose folder for ${workspace.name}`} onCancel={recovery.cancel} onSelect={path => { void recovery.relocate(path); }} />}
  </>;
}
