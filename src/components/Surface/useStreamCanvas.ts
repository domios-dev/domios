import { useLayoutEffect, useEffect, useRef, type RefObject } from "react";
import { createLatestWins } from "./latestWins";
import { decodeJpegFrame, JPEG_DECODE_BACKEND, type DecodedFrame, type FrameDecoder, type FrameFeed } from "./frameFeed";
import { DEV_BUILD } from "../../lib/devBuild";
import { createStreamFrameCapture } from "./streamFrameCapture";
import {
  canvasToPage,
  letterboxRect,
  sameViewport,
  stageRectToPage,
  viewportRequest,
  type FrameMetadata,
  type PagePoint,
  type PageRect,
  type Rect,
  type Size,
  type StreamViewport,
} from "./streamModel";
import type { StreamFrame } from "./surfaceStreamTransport";

export interface StreamCanvasHandle {
  mapPoint(clientX: number, clientY: number): PagePoint | null;
  localPoint(clientX: number, clientY: number): { x: number; y: number };
  stageSize(): Size;
  /** The drawn frame inside the stage, in stage CSS px. */
  drawRect(): Rect;
  pageRect(start: { x: number; y: number }, end: { x: number; y: number }): PageRect | null;
}

interface UseStreamCanvasOptions {
  visible: boolean;
  canvasRef: RefObject<HTMLCanvasElement | null>;
  stageRef: RefObject<HTMLDivElement | null>;
  frames: FrameFeed;
  decode: FrameDecoder;
  onViewport(viewport: StreamViewport): void;
  onHidden?(): void;
  onError(error: unknown): void;
}

const EMPTY_RECT: Rect = { x: 0, y: 0, width: 0, height: 0 };

function currentDpr(): number {
  return typeof window === "undefined" ? 1 : window.devicePixelRatio || 1;
}

export function useStreamCanvas({ visible, canvasRef, stageRef, frames, decode, onViewport, onHidden, onError }: UseStreamCanvasOptions): StreamCanvasHandle {
  const css = useRef<Size>({ width: 0, height: 0 });
  const dpr = useRef(currentDpr());
  const drawCss = useRef<Rect>(EMPTY_RECT);
  const frame = useRef<DecodedFrame | null>(null);
  const meta = useRef<FrameMetadata | null>(null);
  const reported = useRef<StreamViewport | null>(null);
  const capture = useRef<ReturnType<typeof createStreamFrameCapture> | null>(null);
  const callbacks = useRef({ onViewport, onHidden, onError });
  callbacks.current = { onViewport, onHidden, onError };

  const paint = (): boolean => {
    const canvas = canvasRef.current;
    const decoded = frame.current;
    if (!canvas) return false;
    drawCss.current = letterboxRect(decoded ?? { width: 0, height: 0 }, css.current);
    if (!decoded) return false;
    if (canvas.width !== decoded.width) canvas.width = decoded.width;
    if (canvas.height !== decoded.height) canvas.height = decoded.height;
    const context = canvas.getContext("2d");
    if (!context) return false;
    context.imageSmoothingQuality = "high";
    context.drawImage(decoded.image, 0, 0, decoded.width, decoded.height);
    return true;
  };
  const paintRef = useRef(paint);
  paintRef.current = paint;

  useEffect(() => {
    if (!DEV_BUILD || !canvasRef.current || !stageRef.current) return;
    const local = createStreamFrameCapture(canvasRef.current, stageRef.current, decode === decodeJpegFrame ? JPEG_DECODE_BACKEND : "custom");
    capture.current = local;
    return () => {
      capture.current = null;
      local.dispose();
    };
  }, [canvasRef, stageRef, decode]);

  useEffect(() => {
    let active = true;
    const queue = createLatestWins<StreamFrame>(async (next) => {
      const decoded = await decode(next);
      if (!active || (frames.accepts && !frames.accepts(next))) {
        decoded.close();
        return;
      }
      frame.current?.close();
      frame.current = decoded;
      meta.current = next.metadata;
      const drawn = paintRef.current();
      if (DEV_BUILD && drawn) capture.current?.capture(next, decoded, css.current, reported.current, dpr.current);
    }, (error) => callbacks.current.onError(error));
    const unsubscribe = frames.subscribe((next) => queue.push(next));
    return () => {
      active = false;
      unsubscribe();
      queue.dispose();
    };
  }, [frames, decode]);

  useEffect(() => () => {
    frame.current?.close();
    frame.current = null;
  }, []);

  useLayoutEffect(() => {
    if (!visible) return;
    const stage = stageRef.current;
    if (!stage) return;
    const box = stage.getBoundingClientRect();
    if (box.width > 0 && box.height > 0) {
      css.current = { width: box.width, height: box.height };
      dpr.current = currentDpr();
      paintRef.current();
      const request = viewportRequest(css.current, dpr.current);
      if (request) {
        reported.current = request;
        callbacks.current.onViewport(request);
      }
    }
  }, [visible, stageRef]);

  useEffect(() => {
    const stage = stageRef.current;
    if (!stage || typeof ResizeObserver === "undefined") return;
    let scheduled = 0;
    const apply = () => {
      scheduled = 0;
      dpr.current = currentDpr();
      const request = viewportRequest(css.current, dpr.current);
      if (request) drawCss.current = letterboxRect(frame.current ?? { width: 0, height: 0 }, css.current);
      if (!request) {
        // The shared server viewport may change while hidden, so the next visible size must be re-sent.
        if (reported.current) {
          reported.current = null;
          callbacks.current.onHidden?.();
        }
      } else if (!sameViewport(reported.current, request)) {
        reported.current = request;
        callbacks.current.onViewport(request);
      }
    };
    const schedule = () => {
      if (!scheduled) scheduled = requestAnimationFrame(apply);
    };
    const observer = new ResizeObserver((entries) => {
      const box = entries[entries.length - 1]?.contentRect;
      if (box) css.current = { width: box.width, height: box.height };
      schedule();
    });
    observer.observe(stage);
    let media: MediaQueryList | null = null;
    // DPR changes (moving between monitors) never resize the element, so ResizeObserver stays silent.
    const watchDpr = () => {
      media?.removeEventListener("change", onDprChange);
      media = typeof window.matchMedia === "function" ? window.matchMedia(`(resolution: ${currentDpr()}dppx)`) : null;
      media?.addEventListener("change", onDprChange);
    };
    const onDprChange = () => {
      watchDpr();
      schedule();
    };
    watchDpr();
    return () => {
      observer.disconnect();
      media?.removeEventListener("change", onDprChange);
      if (scheduled) cancelAnimationFrame(scheduled);
    };
  }, [stageRef]);

  return {
    mapPoint(clientX, clientY) {
      const canvas = canvasRef.current;
      if (!canvas) return null;
      return canvasToPage(clientX, clientY, canvas.getBoundingClientRect(), drawCss.current, meta.current, css.current);
    },
    localPoint(clientX, clientY) {
      const rect = canvasRef.current?.getBoundingClientRect();
      return { x: clientX - (rect?.left ?? 0), y: clientY - (rect?.top ?? 0) };
    },
    stageSize: () => css.current,
    drawRect: () => drawCss.current,
    pageRect: (start, end) => stageRectToPage(start, end, drawCss.current, meta.current),
  };
}
