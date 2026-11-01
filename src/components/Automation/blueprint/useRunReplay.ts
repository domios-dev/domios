import { useEffect, useLayoutEffect, useState } from "react";
import { replayStep, type ReplayAction, type ReplayState } from "./replayModel";

export function useRunReplay(run: string | null, total: number) {
  const [saved, setSaved] = useState<ReplayState & { run: string | null }>({ run: null, step: total, playing: false });
  const state = saved.run === run ? { step: Math.min(saved.step, total), playing: saved.playing && total > 0 } : { step: total, playing: false };
  const dispatch = (action: ReplayAction) => setSaved((current) => ({ run, ...replayStep(current.run === run ? current : { step: total, playing: false }, action, total) }));
  useLayoutEffect(() => { setSaved({ run, step: total, playing: false }); }, [run, total]);
  useEffect(() => {
    if (!run || !state.playing) return;
    let start: number | null = null;
    let request = 0;
    const frame = (at: number) => {
      start ??= at;
      if (at - start >= 900) {
        setSaved((current) => current.run === run ? { run, ...replayStep(current, { type: "advance" }, total) } : current);
        start = at;
      }
      request = requestAnimationFrame(frame);
    };
    request = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(request);
  }, [run, state.playing, total]);
  return { ...state, dispatch };
}
