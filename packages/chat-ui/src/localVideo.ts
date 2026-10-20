import { useEffect, useState } from "react";
import { loadLocalArtifact } from "./localArtifactLoader";
import { useLocalArtifactTabId } from "./LocalArtifactScope";

interface LocalVideoState {
  path: string;
  src?: string;
  failed?: boolean;
}

export interface LocalVideo {
  src: string | undefined;
  failed: boolean;
}

/** Reads a local video through the host preview loader, only while `enabled`, so a closed chip never pulls megabytes. */
export function useLocalVideo(path: string | undefined, enabled: boolean): LocalVideo {
  const tabId = useLocalArtifactTabId();
  const [state, setState] = useState<LocalVideoState | null>(null);
  useEffect(() => {
    if (!path || !enabled) return;
    let current = true;
    loadLocalArtifact(path, "video", tabId).then(
      (file) => { if (current) setState(file.data.startsWith("data:video/") ? { path, src: file.data } : { path, failed: true }); },
      () => { if (current) setState({ path, failed: true }); },
    );
    return () => { current = false; };
  }, [path, tabId, enabled]);
  const own = path !== undefined && state?.path === path ? state : null;
  return { src: own?.failed ? undefined : own?.src, failed: own?.failed === true };
}
