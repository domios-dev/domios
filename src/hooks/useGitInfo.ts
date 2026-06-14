import { useState, useEffect } from "react";
import { invoke } from "../lib/bridge";

export interface GitInfo {
  branch: string | null;
  is_dirty: boolean;
  repo_root: string;
}

function gitInfoEquals(a: GitInfo | null, b: GitInfo | null): boolean {
  return a?.branch === b?.branch && a?.is_dirty === b?.is_dirty && a?.repo_root === b?.repo_root;
}

export function useGitInfo(paneId: string | null): GitInfo | null {
  const [info, setInfo] = useState<GitInfo | null>(null);

  useEffect(() => {
    if (!paneId) return;

    const fetchInfo = () => {
      invoke<GitInfo | null>("get_pane_git_info", { paneId })
        .then((next) => setInfo((prev) => (gitInfoEquals(prev, next) ? prev : next)))
        .catch(() => setInfo((prev) => (prev === null ? prev : null)));
    };

    fetchInfo();
    const interval = setInterval(fetchInfo, 5000); // refresh every 5s
    return () => clearInterval(interval);
  }, [paneId]);

  return info;
}
