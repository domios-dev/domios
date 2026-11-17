import { frameMatchesViewport, type FrameMetadata, type StreamViewport } from "./streamModel";

export function acceptViewportFrame(metadata: FrameMetadata, viewport: StreamViewport | null): boolean {
  if (!viewport) return false;
  const clamped = { ...viewport, w: Math.max(320, Math.min(7680, viewport.w)), h: Math.max(240, Math.min(4320, viewport.h)) };
  return frameMatchesViewport(metadata, clamped);
}
