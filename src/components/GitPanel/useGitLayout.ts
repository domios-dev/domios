import { useEffect, useRef, useState } from "react";

export type GitLayoutMode = "wide" | "narrow";

const WIDE_MIN = 900;

// Drives layout off the panel's own width (mounted inset-0) via ResizeObserver,
// not the window — container-query semantics without the CSS plumbing.
export function useGitLayout(): { ref: React.RefObject<HTMLDivElement | null>; mode: GitLayoutMode } {
  const ref = useRef<HTMLDivElement>(null);
  const [mode, setMode] = useState<GitLayoutMode>("wide");

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const apply = (w: number) => setMode(w >= WIDE_MIN ? "wide" : "narrow");
    apply(el.clientWidth);
    const ro = new ResizeObserver((entries) => apply(entries[0].contentRect.width));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  return { ref, mode };
}
