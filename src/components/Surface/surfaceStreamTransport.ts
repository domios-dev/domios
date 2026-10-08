import type { FrameMetadata, StreamInput, StreamViewport } from "./streamModel";
import type { SurfaceState } from "./surfaceChromeModel";

export interface StreamFrame {
  data: string | ArrayBuffer;
  metadata: FrameMetadata;
}

export interface SurfaceNotice {
  text: string;
  tone?: "info" | "error";
}

export interface StreamHandlers {
  frame(frame: StreamFrame): void;
  state(state: SurfaceState): void;
  notice(notice: SurfaceNotice): void;
  open(): void;
}

export type Disconnect = () => void;

export interface SurfaceStreamTransport {
  connect(params: StreamViewport, handlers: StreamHandlers): Disconnect;
  invoke<T = unknown>(method: string, args?: Record<string, unknown>): Promise<T>;
}

export const SURFACE_METHOD = {
  resize: "resize",
  input: "input",
  screenshot: "screenshot",
  grabHover: "grab.hover",
  grabSelect: "grab.select",
  grabSend: "grab.send",
  grabCancel: "grab.cancel",
} as const;

export interface ScreenshotResult {
  base64: string;
  path: string;
}

export function resizeArgs(viewport: StreamViewport): Record<string, unknown> {
  return { width: viewport.w, height: viewport.h, dpr: viewport.dpr };
}

export function inputArgs(input: StreamInput): Record<string, unknown> {
  return { ...input };
}
