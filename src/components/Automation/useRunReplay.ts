import { useEffect, useState } from "react";
import { replayFrame } from "./runLogModel";

export interface ReplaySelection { key: string; edges: readonly string[] }

export function useRunReplay(selection: ReplaySelection | null) {
  const [motion, setMotion] = useState(() => typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    const changed = () => setMotion(query.matches);
    changed();
    query.addEventListener("change", changed);
    return () => query.removeEventListener("change", changed);
  }, []);
  useEffect(() => {
    setElapsed(0);
    if (!selection || motion || !selection.edges.length) return;
    let start: number | null = null;
    let request = 0;
    const frame = (at: number) => {
      start ??= at;
      const next = at - start;
      setElapsed(next);
      if (next < selection.edges.length * 850) request = requestAnimationFrame(frame);
    };
    request = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(request);
  }, [selection, motion]);
  return replayFrame(selection?.edges ?? [], elapsed, motion);
}
