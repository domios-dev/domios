import { useCallback, useRef, useState, type ReactNode } from "react";
import { nativeBridge, nativeFolderDialog } from "../lib/nativeBridge";
import { RemoteFolderPicker } from "../components/shared/RemoteFolderPicker";

interface PendingPick {
  title: string;
  initialPath?: string | null;
}

/** The native folder dialog where the host has one, the daemon-backed folder browser everywhere else. */
export function useFolderPicker(): { pickFolder: (title: string, initialPath?: string | null) => Promise<string | null>; folderPicker: ReactNode } {
  const [pending, setPending] = useState<PendingPick | null>(null);
  const resolveRef = useRef<((path: string | null) => void) | null>(null);

  const settle = useCallback((path: string | null) => {
    const resolve = resolveRef.current;
    resolveRef.current = null;
    setPending(null);
    resolve?.(path);
  }, []);

  const pickFolder = useCallback(async (title: string, initialPath?: string | null) => {
    if (nativeBridge.dialogs) {
      return nativeFolderDialog({ directory: true, multiple: false, title }).catch(() => null);
    }
    resolveRef.current?.(null);
    return new Promise<string | null>((resolve) => {
      resolveRef.current = resolve;
      setPending({ title, initialPath });
    });
  }, []);

  const folderPicker = pending
    ? <RemoteFolderPicker title={pending.title} initialPath={pending.initialPath} onCancel={() => settle(null)} onSelect={settle} />
    : null;
  return { pickFolder, folderPicker };
}
