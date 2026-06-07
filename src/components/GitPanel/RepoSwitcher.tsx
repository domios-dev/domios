import { useEffect, useRef, useState } from "react";
import { ChevronDown } from "lucide-react";
import type { RepoEntry } from "./gitApi";

interface RepoSwitcherProps {
  repos: RepoEntry[];
  activePath: string;
  onSelect: (path: string) => void;
}

// Compact repo dropdown (name + dimmed branch) shown when cwd holds >1 repo.
export function RepoSwitcher({ repos, activePath, onSelect }: RepoSwitcherProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const active = repos.find((r) => r.path === activePath) ?? repos[0];

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDoc);
    return () => window.removeEventListener("mousedown", onDoc);
  }, [open]);

  return (
    <div ref={ref} className="ct-git-repos shrink-0">
      <button
        type="button"
        className="ct-git-repos-trigger"
        onClick={() => setOpen((v) => !v)}
        title="Switch repository"
      >
        <span className="name">{active.name}</span>
        {active.branch && <span className="branch">{active.branch}</span>}
        <ChevronDown size={12} className="opacity-60" />
      </button>
      {open && (
        <div className="ct-git-repos-menu">
          {repos.map((r) => (
            <button
              key={r.path}
              type="button"
              className={`ct-git-repos-item${r.path === active.path ? " is-active" : ""}`}
              onClick={() => {
                setOpen(false);
                if (r.path !== activePath) onSelect(r.path);
              }}
            >
              <span className="name">{r.name}</span>
              {r.branch && <span className="branch">{r.branch}</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
