import type { FrameMetadata, Rect, Size } from "./streamModel";

export interface ViewportFrame {
  source: Rect;
  metadata: FrameMetadata;
}

export function viewportFrame(frame: Size, metadata: FrameMetadata): ViewportFrame {
  const source = { x: 0, y: 0, width: frame.width, height: frame.height };
  const { offsetTop = 0, deviceHeight } = metadata;
  if (!Number.isFinite(offsetTop) || !Number.isFinite(deviceHeight)
    || offsetTop <= 0 || offsetTop >= deviceHeight || frame.height <= 0) {
    return { source, metadata };
  }
  const top = Math.round(frame.height * offsetTop / deviceHeight);
  if (top <= 0 || top >= frame.height) return { source, metadata };
  return {
    source: { ...source, y: top, height: frame.height - top },
    metadata: { ...metadata, offsetTop: 0, deviceHeight: deviceHeight - offsetTop },
  };
}
