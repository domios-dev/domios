import { useCallback, useEffect, useRef } from "react";
import type { DotMatrixCanvasHandle } from "./DotMatrixCanvas";

/** Wires a pulsing BrandMark to a DotMatrixCanvas and releases one closing wave when busy work succeeds. */
export function useBusyWaves(busy: boolean, failed: boolean) {
  const matrix = useRef<DotMatrixCanvasHandle>(null);
  const emitWave = useCallback((releasedAt: number) => matrix.current?.emitRelease(releasedAt), []);
  const wasBusy = useRef(busy);
  useEffect(() => {
    if (wasBusy.current && !busy && !failed) emitWave(performance.now());
    wasBusy.current = busy;
  }, [busy, failed, emitWave]);
  return { matrix, emitWave };
}
