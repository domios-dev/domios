import { frameMatchesViewport, sameViewport, type FrameMetadata, type StreamViewport } from "./streamModel";

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

export function acceptViewportFrame(metadata: FrameMetadata, viewport: StreamViewport | null): boolean {
  if (!viewport) return false;
  const clamped = { ...viewport, w: Math.max(320, Math.min(7680, viewport.w)), h: Math.max(240, Math.min(4320, viewport.h)) };
  return frameMatchesViewport(metadata, clamped);
}
