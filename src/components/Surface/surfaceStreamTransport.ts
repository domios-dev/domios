import { type FrameMetadata, type StreamInput, type StreamViewport } from "./streamModel";
import type { SurfaceState } from "./surfaceChromeModel";

export interface StreamFrame {
  data: string | ArrayBuffer;
  metadata: FrameMetadata;
}

export const NOTICE_KIND = { previewPaused: "preview_paused", unavailable: "unavailable" } as const;

export type SurfaceNoticeKind = import("../../../gen/SurfaceNoticeKind").SurfaceNoticeKind;

export type SurfaceNotice = import("../../../gen/SurfaceNotice").SurfaceNotice;

/** A recurring condition the canvas shows in place; it never belongs in the notice bar. */
export function isPreviewPaused(notice: SurfaceNotice | null | undefined): notice is SurfaceNotice & { kind: "preview_paused" } {
  return notice?.kind === NOTICE_KIND.previewPaused;
}

export interface StreamHandlers {
  frame(frame: StreamFrame): void;
  state(state: SurfaceState): void;
  notice(notice: SurfaceNotice): void;
  open(): void;
}

export type Disconnect = () => void;

export interface SurfaceStreamTransport {
  connect(params: StreamViewport, handlers: StreamHandlers, viewer?: string, viewerTarget?: string): Disconnect;
  invoke<T = unknown>(method: string, args?: Record<string, unknown>): Promise<T>;
}

export const SURFACE_METHOD = {
  resize: "resize",
  input: "input",
  grabHover: "grab.hover",
  grabSelect: "grab.select",
  grabSend: "grab.send",
  grabCancel: "grab.cancel",
  snipCapture: "snip.capture",
  snipSend: "snip.send",
  snipCancel: "snip.cancel",
} as const;

export function resizeArgs(viewport: StreamViewport): Record<string, unknown> {
  return { width: viewport.w, height: viewport.h, dpr: viewport.dpr };
}

export function inputArgs(input: StreamInput): Record<string, unknown> {
  return { ...input };
}
