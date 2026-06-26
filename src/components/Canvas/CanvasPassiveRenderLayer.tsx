import { useLayoutEffect, useMemo, useRef } from "react";
import type { CanvasScene } from "./scene";
import {
  drawPassiveScene,
  readPassivePalette,
  publishPassiveCanvasDebug,
  type PassiveRenderFocus,
} from "./passiveRenderer";
import { passiveRendererEnabled } from "./passiveRendererFlag";
import type { ViewportTransform } from "./useCanvasViewport";

export interface CanvasPassiveRenderLayerProps {
  scene: CanvasScene;
  transform: ViewportTransform;
  width: number;
  height: number;
  hostRef?: React.RefObject<HTMLElement | null>;
  focusedNodeId: string | null;
  dominantNodeId: string | null;
  hoveredTask: string | null;
  highlight: { sectionId: string; taskIds: Set<string> } | null;
  tagFilterMatchIds: Set<string> | null;
  blinkIds: ReadonlySet<string>;
  pulsePlanIds: ReadonlySet<string>;
  tagFilterActive?: boolean;
}

export function CanvasPassiveRenderLayer({
  scene,
  transform,
  width,
  height,
  hostRef,
  focusedNodeId,
  dominantNodeId,
  hoveredTask,
  highlight,
  tagFilterMatchIds,
  blinkIds,
  pulsePlanIds,
  tagFilterActive = false,
}: CanvasPassiveRenderLayerProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const focus = useMemo<PassiveRenderFocus>(
    () => ({
      focusedNodeId,
      dominantNodeId,
      hoveredTask,
      highlightTaskIds: highlight?.taskIds ?? null,
      tagFilterMatchIds,
      blinkIds,
      pulsePlanIds,
    }),
    [focusedNodeId, dominantNodeId, hoveredTask, highlight, tagFilterMatchIds, blinkIds, pulsePlanIds],
  );
  const focusKey = useMemo(
    () =>
      [
        focus.focusedNodeId,
        focus.dominantNodeId,
        focus.hoveredTask,
        focus.highlightTaskIds ? [...focus.highlightTaskIds].sort().join(",") : "",
        focus.tagFilterMatchIds ? [...focus.tagFilterMatchIds].sort().join(",") : "",
        [...focus.blinkIds].sort().join(","),
        [...focus.pulsePlanIds].sort().join(","),
      ].join("|"),
    [focus],
  );

  useLayoutEffect(() => {
    if (!passiveRendererEnabled()) return;
    const canvas = canvasRef.current;
    if (!canvas || width < 1 || height < 1) return;
    const dpr = typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1;
    const pxW = Math.max(1, Math.round(width * dpr));
    const pxH = Math.max(1, Math.round(height * dpr));
    if (canvas.width !== pxW || canvas.height !== pxH) {
      canvas.width = pxW;
      canvas.height = pxH;
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
    }
    const ctx = canvas.getContext("2d", { alpha: true });
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const host = hostRef?.current ?? canvas.parentElement;
    if (!host) return;
    const palette = readPassivePalette(host);
    drawPassiveScene(ctx, scene, transform, width, height, palette, focus, tagFilterActive);
    publishPassiveCanvasDebug(canvas, scene, transform);
  }, [scene, transform, width, height, focus, focusKey, tagFilterActive, hostRef]);

  if (!passiveRendererEnabled()) return null;

  return (
    <canvas
      ref={canvasRef}
      data-testid="canvas-render-layer"
      className="pointer-events-none absolute inset-0"
      style={{ zIndex: 1 }}
      aria-hidden
    />
  );
}
