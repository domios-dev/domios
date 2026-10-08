import { useEffect, useMemo, useState } from "react";
import { REPLAY_STEP_MS, replayFrame, type ReplayFrame, type ReplayStep } from "./blueprintRunModel";

function prefersReducedMotion() {
  return typeof window !== "undefined" && typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** Drives one replay by animation frames; `nonce` restarts it, and reduced motion shows the whole path at once. */
export function useBlueprintReplay(steps: readonly ReplayStep[] | null, nonce: number): ReplayFrame | null {
  const [reduced, setReduced] = useState(prefersReducedMotion);
  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    const changed = () => setReduced(query.matches);
    query.addEventListener("change", changed);
    return () => query.removeEventListener("change", changed);
  }, []);
  useEffect(() => {
    setElapsed(0);
    if (!steps || reduced || !steps.length) return;
    const total = steps.length * REPLAY_STEP_MS;
    let start: number | null = null;
    let request = 0;
    const frame = (at: number) => {
      start ??= at;
      const next = at - start;
      setElapsed(next);
      if (next < total) request = requestAnimationFrame(frame);
    };
    request = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(request);
  }, [steps, reduced, nonce]);
  return useMemo(() => steps ? replayFrame(steps, elapsed, reduced) : null, [steps, elapsed, reduced]);
}
