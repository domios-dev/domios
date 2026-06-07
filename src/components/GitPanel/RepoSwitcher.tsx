import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ChevronDown } from "lucide-react";
import type { RepoEntry } from "./gitApi";

interface RepoSwitcherProps {
  repos: RepoEntry[];
  activePath: string;
  onSelect: (path: string) => void;
}

interface MenuPos {
  top: number;
  left: number;
  minWidth: number;
}

// Compact repo dropdown (name + dimmed branch) shown when cwd holds >1 repo.
// Menu renders through a portal so the toolbar's overflow-x:auto can't clip it.
export function RepoSwitcher({ repos, activePath, onSelect }: RepoSwitcherProps) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<MenuPos | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const active = repos.find((r) => r.path === activePath) ?? repos[0];

  const place = useCallback(() => {
    const el = triggerRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    setPos({ top: rect.bottom + 4, left: rect.left, minWidth: rect.width });
  }, []);

  useLayoutEffect(() => {
    if (open) place();
  }, [open, place]);

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    const onPointer = (e: PointerEvent) => {
      const t = e.target as Node;
      if (menuRef.current?.contains(t) || triggerRef.current?.contains(t)) return;
      setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
      }
    };
    window.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
    };
  }, [open]);

  return (
    <div className="ct-git-repos shrink-0">
      <button
        ref={triggerRef}
        type="button"
        className="ct-git-repos-trigger"
        onClick={() => setOpen((v) => !v)}
        title="Switch repository"
      >
        <span className="name">{active.name}</span>
        {active.branch && <span className="branch">{active.branch}</span>}
        <ChevronDown size={12} className="opacity-60" />
      </button>
      {open &&
        pos &&
        createPortal(
          <div
            ref={menuRef}
            className="ct-git-repos-menu"
            style={{ top: pos.top, left: pos.left, minWidth: pos.minWidth }}
          >
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
          </div>,
          document.body,
        )}
    </div>
  );
}
