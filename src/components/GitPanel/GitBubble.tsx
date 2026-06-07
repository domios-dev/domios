import "./styles.css";
import type { GitInfo } from "../../hooks/useGitInfo";

interface GitBubbleProps {
  info: GitInfo | null;
  // Pixel offset from the right edge so it sits just left of the UsageBubble.
  rightOffset: number;
  onClick: (repoRoot: string) => void;
}

export function GitBubble({ info, rightOffset, onClick }: GitBubbleProps) {
  if (!info || !info.branch) return null;

  return (
    <div className="ct-git" style={{ position: "absolute", inset: 0, pointerEvents: "none" }}>
      <button
        type="button"
        className="ct-git-bubble"
        style={{ right: rightOffset, pointerEvents: "auto" }}
        aria-label={`git branch ${info.branch}${info.is_dirty ? " (dirty)" : ""}`}
        onClick={(e) => {
          e.stopPropagation();
          onClick(info.repo_root);
        }}
      >
        <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
          <circle cx="3" cy="3" r="1.6" />
          <circle cx="3" cy="9" r="1.6" />
          <circle cx="9" cy="4.5" r="1.6" />
          <path d="M3 4.6v2.8M3 7.4q0-2.9 6-2.9" />
        </svg>
        <span className="branch">{info.branch}</span>
        {info.is_dirty && <span className="dirty" />}
      </button>
    </div>
  );
}
