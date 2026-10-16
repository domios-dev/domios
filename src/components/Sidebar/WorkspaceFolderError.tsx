import { useState, type ReactNode } from "react";
import { ErrorNotice, InlineErrorNotice } from "../shared/ErrorNotice";
import { nativeBridge, nativeFolderDialog } from "../../lib/nativeBridge";
import type { Workspace } from "../../lib/types";
import { useMultiplexer } from "../../store/multiplexer";
import { RemoteFolderPicker } from "../shared/RemoteFolderPicker";

export function WorkspaceFolderError({ workspace, rawError, cwd, onRelocate, renderNotice }: { workspace: Workspace; rawError?: string | null; cwd?: string | null; onRelocate?: (path: string) => Promise<unknown>; renderNotice?: (chooseFolder: () => void, busy: boolean) => ReactNode }) {
  const { setWorkspaceCwd } = useMultiplexer();
  const [busy, setBusy] = useState(false);
  const [remotePicker, setRemotePicker] = useState(false);
  const [error, setError] = useState<unknown>(null);

  const relocate = async (path: string) => {
    setRemotePicker(false);
    setBusy(true);
    setError(null);
    try {
      if (onRelocate) await onRelocate(path);
      else await setWorkspaceCwd(workspace.id, path);
    } catch (error) {
      setError(error);
    } finally {
      setBusy(false);
    }
  };

  const chooseFolder = async () => {
    if (!nativeBridge.dialogs) {
      setRemotePicker(true);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const path = await nativeFolderDialog({
        directory: true, multiple: false, title: `Choose folder for ${workspace.name}`,
      });
      if (path) await relocate(path);
    } catch (error) {
      setError(error);
    } finally {
      setBusy(false);
    }
  };

  return <section aria-label={`Workspace ${workspace.name} folder error`} className={`flex h-full min-w-0 w-full flex-col items-center justify-center gap-4 bg-[var(--ct-bg)] text-center ${renderNotice ? "" : "p-6"}`}>
    {renderNotice ? renderNotice(() => { void chooseFolder(); }, busy) : <ErrorNotice error={{ code: "workspace_cwd_missing", raw: rawError ?? `Workspace folder unavailable: ${workspace.cwd}` }} cwd={cwd ?? workspace.cwd} blocking
      actions={[{ label: "Choose folder", disabled: busy, onClick: () => { void chooseFolder(); } }]} />}
    {error != null && <InlineErrorNotice error={error} operation="choosing a workspace folder" blocking />}
    {remotePicker && <RemoteFolderPicker title={`Choose folder for ${workspace.name}`} onCancel={() => setRemotePicker(false)} onSelect={(path) => { void relocate(path); }} />}
  </section>;
}
