import type { Tab, Workspace } from "../../lib/types";
import { isReadOnlyTab } from "../../lib/tabInputPolicy";
import { tabFailureDescription, type TabFailure } from "../../lib/tabFailure";
import { describeError } from "../../lib/describeError";
import { RuntimeTranscriptNotice } from "./TranscriptNotice";
import { useMuxSelector } from "../../store/multiplexer/selectorStore";
import { ErrorNotice, InlineErrorNotice, type ErrorNoticeAction } from "../shared/ErrorNotice";
import { paneStateModel } from "../shared/paneStateModel";
import { RemoteFolderPicker } from "../shared/RemoteFolderPicker";
import { useFolderRecovery } from "../shared/useFolderRecovery";
import { relocateTabFolder } from "./TabLaunchFailure";
import { useTabRecovery } from "./useTabRecovery";

/** A launched session's recovery at the end of its transcript. */
export function TabFailureNotice({ tab, failure, detail, workspaceId }: {
  tab: Tab;
  failure: TabFailure;
  detail: string | null;
  workspaceId: string | null;
}) {
  const { actions, error, restart } = useTabRecovery(tab.id, isReadOnlyTab(tab) ? null : failure.recovery);
  const workspace = useMuxSelector(state => state.workspaces.find(candidate => candidate.id === workspaceId));
  const description = tabFailureDescription(failure, detail);
  const missingFolder = !isReadOnlyTab(tab) && paneStateModel(detail).actions.includes("choose-folder");
  return <div className="w-full">
    {missingFolder && workspace
      ? <FolderRecoveryNotice tab={tab} workspace={workspace} detail={detail} failure={failure} actions={actions} />
      : <RuntimeTranscriptNotice
        sentence={description?.cause.split(". ")[0].concat(".") ?? describeError(detail, { operation: "resuming this session" }).title}
        details={[detail, error].filter(Boolean).join("\n")}
        action={actions.length ? { label: "Restart the agent", loading: restart.restarting, run: () => { void restart.restart(); } } : undefined}
      />}
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
