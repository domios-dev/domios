import type { ReactNode } from "react";
import type { ViewportBox } from "@codeterm/canvas-camera";
import { CanvasToolbar } from "./CanvasToolbar";
import { useZoomCanvas, type CanvasFocus } from "./useZoomCanvas";

export function ZoomCanvas({ bounds, focus, children, onNavigate, onSelect, wheelBehavior = "zoom", fitLabel = "Fit all", resetLabel = "Reset to 100%" }: {
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
  return <div className="relative min-h-0 flex-1">
    <div ref={camera.containerRef} {...camera.handlers} {...interactions} role="region" aria-label="Zoomable canvas" tabIndex={0} data-fullscreen-autofocus=""
      className="absolute inset-0 overflow-hidden focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]"
      style={{ touchAction: "none", overscrollBehavior: "none", cursor: camera.isDragging ? "grabbing" : "grab" }}>
      <div ref={camera.layerRef} style={{ position: "absolute", left: 0, top: 0, transformOrigin: "0 0" }}>{children}</div>
    </div>
    <CanvasToolbar label="Canvas zoom controls" fitLabel={fitLabel} actualSizeLabel={resetLabel} fitPressed={mode === "fit"} actualSizePressed={mode === "natural"}
      onFit={fit} onActualSize={natural} onZoomOut={() => zoom(-0.2)} onZoomIn={() => zoom(0.25)}
      extras={<output ref={scaleOutput} aria-live="polite" aria-label="Zoom level" className="px-1 font-mono text-xs tabular-nums">{Math.round(camera.transformRef.current.scale * 100)}%</output>} />
  </div>;
}
