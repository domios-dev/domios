import { viewportAction, type StreamViewport } from "./streamModel";

export function streamLifecycleAction(
  visible: boolean,
  connected: boolean,
  sent: StreamViewport | null,
  viewport: StreamViewport | null,
): "connect" | "disconnect" | "resize" | "none" {
  if (!visible || !viewport) return connected ? "disconnect" : "none";
  return viewportAction(connected, sent, viewport);
}
