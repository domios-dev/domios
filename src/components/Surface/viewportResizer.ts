import { completeResize, requestResize, type ResizeState } from "./resizeFrameModel";
import { sameViewport, type StreamViewport } from "./streamModel";

export function createViewportResizer(run: (viewport: StreamViewport) => Promise<unknown>, onError: (error: unknown) => void) {
  let state: ResizeState = { inFlight: null, pending: null };
  let disposed = false;
  let last: StreamViewport | null = null;
  const drain = async () => {
    while (state.inFlight && !disposed) {
      try {
        await run(state.inFlight);
      } catch (error) {
        onError(error);
      }
      state = completeResize(state);
    }
  };
  return {
    push(viewport: StreamViewport) {
      if (disposed || sameViewport(last, viewport)) return;
      last = viewport;
      const busy = state.inFlight !== null;
      state = requestResize(state, viewport);
      if (!busy) void drain();
    },
    dispose() { disposed = true; state = { ...state, pending: null }; },
  };
}
