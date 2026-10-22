import type { ReactNode } from "react";
import type { ViewportBox } from "@codeterm/canvas-camera";
import { useZoomCanvas, type CanvasFocus } from "./useZoomCanvas";
import { ViewerButton } from "./ViewerButton";

export function ZoomCanvas({ bounds, focus, children, onNavigate, onSelect, wheelBehavior = "zoom", fitLabel = "Show all", resetLabel = "Reset to 100%" }: {
  bounds: ViewportBox;
  focus?: CanvasFocus | null;
  children: ReactNode;
  onNavigate?: (direction: number) => void;
  onSelect?: (index: number) => void;
  wheelBehavior?: "pan" | "zoom";
  fitLabel?: string;
  resetLabel?: string;
}) {
  const { camera, mode, scaleOutput, interactions, fit, zoom, natural } = useZoomCanvas({ bounds, focus, wheelBehavior, onNavigate, onSelect });
  return <>
    <div className="flex shrink-0 flex-wrap items-center gap-2 p-1">
      <ViewerButton onClick={fit} aria-label={fitLabel} aria-pressed={mode === "fit"}>{fitLabel === "Show all" ? "Show all" : "Fit"}</ViewerButton>
      <ViewerButton onClick={natural} aria-label={resetLabel} aria-pressed={mode === "natural"}>100%</ViewerButton>
      <ViewerButton onClick={() => zoom(0.25)} aria-label="Zoom in">+</ViewerButton>
      <ViewerButton onClick={() => zoom(-0.2)} aria-label="Zoom out">−</ViewerButton>
      <output ref={scaleOutput} aria-live="polite" aria-label="Zoom level" className="font-mono text-xs tabular-nums">{Math.round(camera.transformRef.current.scale * 100)}%</output>
    </div>
    <div ref={camera.containerRef} {...camera.handlers} {...interactions} role="region" aria-label="Zoomable canvas" tabIndex={0}
      className="relative min-h-0 flex-1 overflow-hidden focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]"
      style={{ touchAction: "none", overscrollBehavior: "none", cursor: camera.isDragging ? "grabbing" : "grab" }}>
      <div ref={camera.layerRef} style={{ position: "absolute", left: 0, top: 0, transformOrigin: "0 0" }}>{children}</div>
    </div>
  </>;
}
