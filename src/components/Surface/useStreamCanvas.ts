import { useLayoutEffect, useEffect, useRef, type RefObject } from "react";
import { createLatestWins } from "./latestWins";
import { type DecodedFrame, type FrameDecoder, type FrameFeed } from "./frameFeed";
import { viewportFrame } from "./streamFrameModel";
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
  const drawCss = useRef<Rect>(EMPTY_RECT);
  const frame = useRef<DecodedFrame | null>(null);
  const drawnSize = useRef<Size>({ width: 0, height: 0 });
  const rawMeta = useRef<FrameMetadata | null>(null);
  const meta = useRef<FrameMetadata | null>(null);
  const reported = useRef<StreamViewport | null>(null);
  const callbacks = useRef({ onViewport, onHidden, onError });
  callbacks.current = { onViewport, onHidden, onError };

  const paint = (): boolean => {
    const canvas = canvasRef.current;
    const decoded = frame.current;
    if (!canvas) return false;
    if (!decoded) return false;
    const cropped = rawMeta.current ? viewportFrame(decoded, rawMeta.current) : null;
    const source = cropped?.source ?? { x: 0, y: 0, width: decoded.width, height: decoded.height };
    drawnSize.current = source;
    meta.current = cropped?.metadata ?? rawMeta.current;
    drawCss.current = letterboxRect(source, css.current);
    if (canvas.width !== source.width) canvas.width = source.width;
    if (canvas.height !== source.height) canvas.height = source.height;
    const context = canvas.getContext("2d");
    if (!context) return false;
    context.imageSmoothingQuality = "high";
    context.drawImage(decoded.image, source.x, source.y, source.width, source.height, 0, 0, source.width, source.height);
    return true;
  };
  const paintRef = useRef(paint);
  paintRef.current = paint;

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
      rawMeta.current = next.metadata;
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
    if (!visible) {
      reported.current = null;
      return;
    }
    const stage = stageRef.current;
    if (!stage) return;
    const measure = (box: Size = stage.getBoundingClientRect()) => {
      css.current = { width: box.width, height: box.height };
      const request = viewportRequest(css.current, currentDpr());
      drawCss.current = letterboxRect(drawnSize.current, css.current);
      if (!request) {
        if (reported.current) {
          reported.current = null;
          callbacks.current.onHidden?.();
        }
      } else if (!sameViewport(reported.current, request)) {
        reported.current = request;
        callbacks.current.onViewport(request);
      }
    };
    measure();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver((entries) => {
      const box = entries[entries.length - 1]?.contentRect;
      if (box) measure(box);
    });
    observer?.observe(stage);
    let media: MediaQueryList | null = null;
    // DPR changes (moving between monitors) never resize the element, so ResizeObserver stays silent.
    const watchDpr = () => {
      media?.removeEventListener("change", onDprChange);
      media = typeof window.matchMedia === "function" ? window.matchMedia(`(resolution: ${currentDpr()}dppx)`) : null;
      media?.addEventListener("change", onDprChange);
    };
    const onDprChange = () => {
      watchDpr();
      measure();
    };
    watchDpr();
    return () => {
      observer?.disconnect();
      media?.removeEventListener("change", onDprChange);
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
