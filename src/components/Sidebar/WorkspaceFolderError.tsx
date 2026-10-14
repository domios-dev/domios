import { useState } from "react";
import { FolderX } from "lucide-react";
import { nativeBridge, nativeFolderDialog } from "../../lib/nativeBridge";
import { spawnErrorMessage } from "../../lib/spawnErrorMessage";
import type { Workspace } from "../../lib/types";
import { useMultiplexer } from "../../store/multiplexer";
import { Button } from "../shared/Button";
import { RemoteFolderPicker } from "../shared/RemoteFolderPicker";

export function WorkspaceFolderError({ workspace }: { workspace: Workspace }) {
  const { setWorkspaceCwd } = useMultiplexer();
  const [busy, setBusy] = useState(false);
  const [remotePicker, setRemotePicker] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const relocate = async (path: string) => {
    setRemotePicker(false);
    setBusy(true);
    setError(null);
    try {
      await setWorkspaceCwd(workspace.id, path);
    } catch (error) {
      setError(spawnErrorMessage(error));
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
      setError(spawnErrorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  return <section aria-label={`Workspace ${workspace.name} folder error`} className="flex h-full flex-col items-center justify-center gap-4 bg-[var(--ct-bg)] p-6 text-center">
    <FolderX aria-hidden size={28} className="text-[var(--ct-red)]" />
    <p role="alert" className="max-w-lg break-words text-sm text-[var(--ct-fg)]">
      Folder not found: {workspace.cwd}. The workspace lost its folder; choose its new location.
    </p>
    <Button variant="primary" loading={busy} onClick={() => { void chooseFolder(); }}>Choose folder</Button>
    {error && <p role="alert" className="max-w-lg break-words text-xs text-[var(--ct-red)]">{error}</p>}
    {remotePicker && <RemoteFolderPicker title={`Choose folder for ${workspace.name}`} onCancel={() => setRemotePicker(false)} onSelect={(path) => { void relocate(path); }} />}
  </section>;
}
