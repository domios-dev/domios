import { useEffect, useState } from "react";
import { loadLocalArtifact } from "./localArtifactLoader";
import { useLocalArtifactTabId } from "./LocalArtifactScope";

interface LocalImageState {
  path: string;
  src?: string;
  failed?: boolean;
}

export interface LocalImage {
  src: string | undefined;
  failed: boolean;
  markFailed: () => void;
  retry: () => void;
}

/** Reads an absolute local image through the host preview loader for the owning tab; a stale path never shows another path's pixels. */
export function useLocalImage(path: string | undefined): LocalImage {
  const tabId = useLocalArtifactTabId();
  const [attempt, setAttempt] = useState(0);
  const [state, setState] = useState<LocalImageState | null>(null);
  useEffect(() => {
    if (!path) return;
    let current = true;
    loadLocalArtifact(path, "image", tabId).then(
      (file) => { if (current) setState(file.data.startsWith("data:image/") ? { path, src: file.data } : { path, failed: true }); },
      () => { if (current) setState({ path, failed: true }); },
    );
    return () => { current = false; };
  }, [path, tabId, attempt]);
  const own = path !== undefined && state?.path === path ? state : null;
  return {
    src: own?.failed ? undefined : own?.src,
    failed: own?.failed === true,
    retry: () => { setState(null); setAttempt((value) => value + 1); },
    markFailed: () => { if (path) setState({ path, failed: true }); },
  };
}
