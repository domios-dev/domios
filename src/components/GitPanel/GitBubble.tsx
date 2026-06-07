import { useEffect, useLayoutEffect, useRef, useState } from "react";
import "./styles.css";
import type { GitInfo } from "../../hooks/useGitInfo";
import type { ApiConfig } from "../../lib/types";
import { fetchRepos, type RepoEntry } from "./gitApi";

interface GitBubbleProps {
  info: GitInfo | null;
  cwd: string | null;
  api: ApiConfig;
  onClick: (repoRoot: string) => void;
}

const GAP = 6;
const EDGE = 8;

export function GitBubble({ info, cwd, api, onClick }: GitBubbleProps) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const [rightOffset, setRightOffset] = useState(EDGE);
  // Multi-repo fallback: the pane cwd is a folder of checkouts, not a repo.
  const [childRepos, setChildRepos] = useState<RepoEntry[]>([]);

  // Sit just left of the UsageBubble, tracking its real width.
  useLayoutEffect(() => {
    const pane = wrapRef.current?.closest("[data-pane-id]");
    const usage = pane?.querySelector(".ct-usage-bubble");
    if (!usage) {
      setRightOffset(EDGE);
      return;
    }
    const update = () => setRightOffset(EDGE + (usage as HTMLElement).offsetWidth + GAP);
    update();
    const ro = new ResizeObserver(update);
    ro.observe(usage);
    return () => ro.disconnect();
  });

  useEffect(() => {
    if (info?.branch || !cwd) {
      setChildRepos([]);
      return;
    }
    let active = true;
    fetchRepos(api, cwd)
      .then((repos) => active && setChildRepos(repos))
      .catch(() => active && setChildRepos([]));
    return () => {
      active = false;
    };
  }, [info?.branch, cwd, api]);

  const isRepo = !!info?.branch;
  if (!isRepo && childRepos.length === 0) return null;

  const label = isRepo ? info!.branch! : `${childRepos.length} repos`;
  const target = isRepo ? info!.repo_root : cwd!;

  return (
    <div
      ref={wrapRef}
      className="ct-git"
      style={{ position: "absolute", inset: 0, pointerEvents: "none" }}
    >
      <button
        type="button"
        className="ct-git-bubble"
        style={{ right: rightOffset, pointerEvents: "auto" }}
        aria-label={
          isRepo
            ? `git branch ${label}${info!.is_dirty ? " (dirty)" : ""}`
            : `${childRepos.length} git repositories`
        }
        onClick={(e) => {
          e.stopPropagation();
          onClick(target);
        }}
      >
        <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
          <circle cx="3" cy="3" r="1.6" />
          <circle cx="3" cy="9" r="1.6" />
          <circle cx="9" cy="4.5" r="1.6" />
          <path d="M3 4.6v2.8M3 7.4q0-2.9 6-2.9" />
        </svg>
        <span className="branch">{label}</span>
        {isRepo && info!.is_dirty && <span className="dirty" />}
      </button>
    </div>
  );
}
