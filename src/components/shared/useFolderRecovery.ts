import { useState } from "react";
import { nativeBridge, nativeFolderDialog } from "../../lib/nativeBridge";
import type { Workspace } from "../../lib/types";
import { useMultiplexer } from "../../store/multiplexer";

export function useFolderRecovery(workspace: Workspace, onRelocate?: (path: string) => Promise<unknown>) {
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

  return { busy, error, remotePicker, relocate, chooseFolder, cancel: () => setRemotePicker(false) };
}
