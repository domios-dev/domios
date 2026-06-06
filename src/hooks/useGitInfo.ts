import { useState, useEffect } from "react";
import { invoke } from "../lib/bridge";

export interface GitInfo {
  branch: string | null;
  is_dirty: boolean;
  repo_root: string;
}

export function useGitInfo(paneId: string | null): GitInfo | null {
  const [info, setInfo] = useState<GitInfo | null>(null);

  useEffect(() => {
    if (!paneId) return;

    const fetchInfo = () => {
      invoke<GitInfo | null>("get_pane_git_info", { paneId })
        .then(setInfo)
        .catch(() => setInfo(null));
    };

    fetchInfo();
    const interval = setInterval(fetchInfo, 5000); // refresh every 5s
    return () => clearInterval(interval);
  }, [paneId]);

  return info;
}
