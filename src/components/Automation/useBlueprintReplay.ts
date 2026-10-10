import { useEffect, useMemo, useState } from "react";
import { REPLAY_STEP_MS, replayFrame, type ReplayFrame, type ReplayStep } from "./blueprintRunModel";
import { useBlueprintMotion } from "./useBlueprintMotion";

/** Drives one replay by animation frames; `nonce` restarts it, and reduced motion shows the whole path at once. */
export function useBlueprintReplay(steps: readonly ReplayStep[] | null, nonce: number | string, stepMs = REPLAY_STEP_MS): ReplayFrame | null {
  const reduced = !useBlueprintMotion();
  const [elapsed, setElapsed] = useState(0);
  const path = JSON.stringify(steps);
  const total = (steps?.length ?? 0) * stepMs;
  useEffect(() => {
    setElapsed(0);
    if (!total || reduced) return;
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
  }, [path, total, reduced, nonce]);
  return useMemo(() => steps ? replayFrame(steps, elapsed, reduced, stepMs) : null, [steps, elapsed, reduced, stepMs]);
}
