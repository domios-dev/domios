import { useState, useEffect, useCallback } from "react";
import { invoke } from "../lib/bridge";

export interface GitInfo {
  branch: string | null;
  is_dirty: boolean;
  repo_root: string;
  added: number;
  deleted: number;
}

function gitInfoEquals(a: GitInfo | null, b: GitInfo | null): boolean {
  return (
    a?.branch === b?.branch &&
    a?.is_dirty === b?.is_dirty &&
    a?.repo_root === b?.repo_root &&
    a?.added === b?.added &&
    a?.deleted === b?.deleted
  );
}

export function useGitInfo(paneId: string | null): { info: GitInfo | null; refresh: () => void } {
  const [info, setInfo] = useState<GitInfo | null>(null);

  const refresh = useCallback(() => {
    if (!paneId) {
      setInfo((prev) => (prev === null ? prev : null));
      return;
    }
    invoke<GitInfo | null>("get_pane_git_info", { paneId })
      .then((next) => setInfo((prev) => (gitInfoEquals(prev, next) ? prev : next)))
      .catch(() => setInfo((prev) => (prev === null ? prev : null)));
  }, [paneId]);

  useEffect(() => {
    refresh();
    if (!paneId) return;
    const interval = setInterval(refresh, 5000);
    return () => clearInterval(interval);
  }, [paneId, refresh]);

  return { info, refresh };
}
