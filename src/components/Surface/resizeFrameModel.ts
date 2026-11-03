import { frameMatchesViewport, requestedFrameSize, sameViewport, type FrameMetadata, type Size, type StreamViewport } from "./streamModel";

export interface ResizeState {
  inFlight: StreamViewport | null;
  pending: StreamViewport | null;
}

export function requestResize(state: ResizeState, viewport: StreamViewport): ResizeState {
  if (state.inFlight) return { ...state, pending: sameViewport(state.inFlight, viewport) ? null : viewport };
  return { inFlight: viewport, pending: null };
}

export function completeResize(state: ResizeState): ResizeState {
  return { inFlight: state.pending, pending: null };
}

export function acceptViewportFrame(metadata: FrameMetadata, viewport: StreamViewport | null, decoded?: Size): boolean {
  if (!viewport) return false;
  const clamped = { ...viewport, w: Math.max(320, Math.min(7680, viewport.w)), h: Math.max(240, Math.min(4320, viewport.h)) };
  const expected = requestedFrameSize({ ...clamped, dpr: Math.max(1, Math.min(3, viewport.dpr)) });
  return frameMatchesViewport(metadata, clamped)
    && (!decoded || (Math.abs(decoded.width - expected.width) <= 1 && Math.abs(decoded.height - expected.height) <= 1));
}
