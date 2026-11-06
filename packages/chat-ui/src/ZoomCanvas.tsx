import type { ReactNode } from "react";
import type { ViewportBox } from "@codeterm/canvas-camera";
import { CanvasToolbar } from "./CanvasToolbar";
import { useZoomCanvas, type CanvasFocus } from "./useZoomCanvas";
import type { InitialCanvasFit } from "./imageCanvasCamera";

export function ZoomCanvas({ bounds, focus, cameraKey, initialFit = "ready", children, onNavigate, onSelect, wheelBehavior = "zoom", fitLabel = "Fit all", resetLabel = "Reset to 100%" }: {
  bounds: ViewportBox;
  focus?: CanvasFocus | null;
  cameraKey?: string;
  initialFit?: InitialCanvasFit;
  children: ReactNode;
  onNavigate?: (direction: number) => void;
  onSelect?: (index: number) => void;
  wheelBehavior?: "pan" | "zoom";
  fitLabel?: string;
  resetLabel?: string;
}) {
  const { camera, mode, scaleOutput, interactions, fit, zoom, natural } = useZoomCanvas({ bounds, focus, cameraKey, initialFit, wheelBehavior, onNavigate, onSelect });
  return <div className="relative min-h-0 flex-1">
    <div ref={camera.containerRef} {...camera.handlers} {...interactions} role="region" aria-label="Zoomable canvas" tabIndex={0} data-fullscreen-autofocus=""
      className="absolute inset-0 overflow-hidden focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]"
      style={{ touchAction: "none", overscrollBehavior: "none", cursor: camera.isDragging ? "grabbing" : "grab",
        backgroundColor: "var(--ct-bg)",
        backgroundImage: "radial-gradient(circle, color-mix(in srgb, var(--ct-accent) 25%, transparent) var(--canvas-dot, 1px), transparent var(--canvas-dot, 1px))",
        backgroundSize: "var(--canvas-step, 24px) var(--canvas-step, 24px)",
        backgroundPosition: "var(--canvas-x, 0px) var(--canvas-y, 0px)",
      }}>
      <div ref={camera.layerRef} style={{ position: "absolute", left: 0, top: 0, transformOrigin: "0 0" }}>{children}</div>
    </div>
    <CanvasToolbar label="Canvas zoom controls" fitLabel={fitLabel} actualSizeLabel={resetLabel} fitPressed={mode === "fit"} actualSizePressed={mode === "natural"}
      onFit={fit} onActualSize={natural} onZoomOut={() => zoom(-0.2)} onZoomIn={() => zoom(0.25)}
      extras={<output ref={scaleOutput} aria-live="polite" aria-label="Zoom level" className="px-1 font-mono text-xs tabular-nums">{Math.round(camera.transformRef.current.scale * 100)}%</output>} />
  </div>;
}
