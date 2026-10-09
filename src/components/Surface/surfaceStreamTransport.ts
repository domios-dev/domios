import { MODIFIER, type FrameMetadata, type StreamInput, type StreamViewport } from "./streamModel";
import type { SurfaceState } from "./surfaceChromeModel";

export interface StreamFrame {
  data: string | ArrayBuffer;
  metadata: FrameMetadata;
}

export const NOTICE_KIND = { previewPaused: "preview_paused" } as const;

export type SurfaceNoticeKind = (typeof NOTICE_KIND)[keyof typeof NOTICE_KIND];

export interface SurfaceNotice {
  text: string;
  tone?: "info" | "warn" | "error";
  kind?: SurfaceNoticeKind;
  detail?: string;
}

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
  connect(params: StreamViewport, handlers: StreamHandlers): Disconnect;
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

// The wire shape is the core `input` contract; the CDP details (code, virtual key) are resolved core-side.
export function inputArgs(input: StreamInput): Record<string, unknown> {
  switch (input.kind) {
    case "click":
      return { kind: "click", x: input.x, y: input.y };
    case "wheel":
      return { kind: "wheel", x: input.x, y: input.y, deltaY: input.deltaY };
    case "text":
      return { kind: "text", text: input.text };
    case "key":
      return {
        kind: "key",
        key: input.key,
        shift: (input.modifiers & MODIFIER.shift) !== 0,
        ctrl: (input.modifiers & MODIFIER.ctrl) !== 0,
        alt: (input.modifiers & MODIFIER.alt) !== 0,
        meta: (input.modifiers & MODIFIER.meta) !== 0,
      };
  }
}
