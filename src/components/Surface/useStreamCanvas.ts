import { useEffect, useRef, type RefObject } from "react";
import { createLatestWins } from "./latestWins";
import type { DecodedFrame, FrameDecoder, FrameFeed } from "./frameFeed";
import {
  canvasToPage,
  layoutStream,
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

export function useStreamCanvas({ canvasRef, stageRef, frames, decode, onViewport, onHidden, onError }: UseStreamCanvasOptions): StreamCanvasHandle {
  const css = useRef<Size>({ width: 0, height: 0 });
  const dpr = useRef(currentDpr());
  const drawCss = useRef<Rect>(EMPTY_RECT);
  const frame = useRef<DecodedFrame | null>(null);
  const meta = useRef<FrameMetadata | null>(null);
  const reported = useRef<StreamViewport | null>(null);
  const callbacks = useRef({ onViewport, onHidden, onError });
  callbacks.current = { onViewport, onHidden, onError };

  const paint = () => {
    const canvas = canvasRef.current;
    const decoded = frame.current;
    if (!canvas) return;
    const layout = layoutStream(decoded ?? { width: 0, height: 0 }, css.current, dpr.current);
    if (canvas.width !== layout.backing.width) canvas.width = layout.backing.width;
    if (canvas.height !== layout.backing.height) canvas.height = layout.backing.height;
    drawCss.current = layout.drawCss;
    const context = canvas.getContext("2d");
    if (!context) return;
    context.fillStyle = "#000";
    context.fillRect(0, 0, layout.backing.width, layout.backing.height);
    if (!decoded || layout.draw.width <= 0) return;
    context.imageSmoothingQuality = "high";
    context.drawImage(decoded.image, layout.draw.x, layout.draw.y, layout.draw.width, layout.draw.height);
  };
  const paintRef = useRef(paint);
  paintRef.current = paint;

  useEffect(() => {
    const queue = createLatestWins<StreamFrame>(async (next) => {
      const decoded = await decode(next);
      frame.current?.close();
      frame.current = decoded;
      meta.current = next.metadata;
      paintRef.current();
    }, (error) => callbacks.current.onError(error));
    const unsubscribe = frames.subscribe((next) => queue.push(next));
    return () => {
      unsubscribe();
      queue.dispose();
    };
  }, [frames, decode]);

  useEffect(() => () => {
    frame.current?.close();
    frame.current = null;
  }, []);

  useEffect(() => {
    const stage = stageRef.current;
    if (!stage || typeof ResizeObserver === "undefined") return;
    let scheduled = 0;
    const apply = () => {
      scheduled = 0;
      dpr.current = currentDpr();
      paintRef.current();
      const request = viewportRequest(css.current, dpr.current);
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
      return canvasToPage(clientX, clientY, canvas.getBoundingClientRect(), drawCss.current, meta.current);
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
