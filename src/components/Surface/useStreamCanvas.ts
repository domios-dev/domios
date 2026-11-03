import { useLayoutEffect, useEffect, useRef, type RefObject } from "react";
import { createLatestWins } from "./latestWins";
import { type DecodedFrame, type FrameDecoder, type FrameFeed } from "./frameFeed";
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
  const callbacks = useRef({ onViewport, onHidden, onError });
  callbacks.current = { onViewport, onHidden, onError };

  const paint = (): boolean => {
    const canvas = canvasRef.current;
    const decoded = frame.current;
    if (!canvas) return false;
    drawCss.current = letterboxRect(decoded ?? { width: 0, height: 0 }, css.current);
    if (!decoded) return false;
    if (meta.current && frames.accepts && !frames.accepts({ data: "", metadata: meta.current }, decoded)) {
      canvas.getContext("2d")?.clearRect(0, 0, canvas.width, canvas.height);
      drawCss.current = EMPTY_RECT;
      return false;
    }
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
    let active = true;
    const queue = createLatestWins<StreamFrame>(async (next) => {
      const decoded = await decode(next);
      if (!active || (frames.accepts && !frames.accepts(next, decoded))) {
        decoded.close();
        return;
      }
      frame.current?.close();
      frame.current = decoded;
      meta.current = next.metadata;
      paintRef.current();
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
    const stage = stageRef.current;
    const hide = () => {
      if (!reported.current) return;
      reported.current = null;
      callbacks.current.onHidden?.();
    };
    if (!visible || !stage) {
      hide();
      return;
    }
    const apply = () => {
      const box = stage.getBoundingClientRect();
      css.current = { width: box.width, height: box.height };
      dpr.current = currentDpr();
      const request = document.visibilityState === "hidden" ? null : viewportRequest(css.current, dpr.current);
      if (!request) {
        hide();
        return;
      }
      if (!sameViewport(reported.current, request)) {
        reported.current = request;
        callbacks.current.onViewport(request);
      }
      paintRef.current();
    };
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(apply);
    observer?.observe(stage);
    let media: MediaQueryList | null = null;
    const watchDpr = () => {
      media?.removeEventListener("change", onDprChange);
      media = typeof window.matchMedia === "function" ? window.matchMedia(`(resolution: ${currentDpr()}dppx)`) : null;
      media?.addEventListener("change", onDprChange);
    };
    const onDprChange = () => {
      watchDpr();
      apply();
    };
    watchDpr();
    const refocus = () => {
      hide();
      apply();
    };
    document.addEventListener("visibilitychange", apply);
    window.addEventListener("focus", refocus);
    apply();
    return () => {
      observer?.disconnect();
      media?.removeEventListener("change", onDprChange);
      document.removeEventListener("visibilitychange", apply);
      window.removeEventListener("focus", refocus);
      hide();
    };
  }, [visible, stageRef]);

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
