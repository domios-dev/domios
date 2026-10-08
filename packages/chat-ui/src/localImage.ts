import { useEffect, useState } from "react";
import { loadLocalArtifact } from "./localArtifactLoader";

interface LocalImageState {
  path: string;
  src?: string;
  failed?: boolean;
}

export interface LocalImage {
  src: string | undefined;
  failed: boolean;
  markFailed: () => void;
}

/** Reads an absolute local image through the host preview loader; a stale path never shows another path's pixels. */
export function useLocalImage(path: string | undefined): LocalImage {
  const [state, setState] = useState<LocalImageState | null>(null);
  useEffect(() => {
    if (!path) return;
    let current = true;
    loadLocalArtifact(path, "image").then(
      (file) => { if (current) setState(file.data.startsWith("data:image/") ? { path, src: file.data } : { path, failed: true }); },
      () => { if (current) setState({ path, failed: true }); },
    );
    return () => { current = false; };
  }, [path]);
  const own = path !== undefined && state?.path === path ? state : null;
  return {
    src: own?.failed ? undefined : own?.src,
    failed: own?.failed === true,
    markFailed: () => { if (path) setState({ path, failed: true }); },
  };
}
