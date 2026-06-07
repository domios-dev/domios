import { useEffect, useRef, useState } from "react";

export type GitLayoutMode = "wide" | "medium" | "narrow";

const WIDE_MIN = 1000;
const MEDIUM_MIN = 560;

function modeFor(w: number): GitLayoutMode {
  if (w >= WIDE_MIN) return "wide";
  if (w >= MEDIUM_MIN) return "medium";
  return "narrow";
}

// Drives layout off the panel's own width (mounted inset-0) via ResizeObserver,
// not the window — container-query semantics without the CSS plumbing.
export function useGitLayout(): { ref: React.RefObject<HTMLDivElement | null>; mode: GitLayoutMode } {
  const ref = useRef<HTMLDivElement>(null);
  const [mode, setMode] = useState<GitLayoutMode>("wide");

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const apply = (w: number) => setMode(modeFor(w));
    apply(el.clientWidth);
    const ro = new ResizeObserver((entries) => apply(entries[0].contentRect.width));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  return { ref, mode };
}
