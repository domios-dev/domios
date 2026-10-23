import type { ReactNode } from "react";
import { Minus, Plus, RefreshCw } from "lucide-react";

export interface CanvasToolbarProps {
  label?: string;
  floating?: boolean;
  view?: ReactNode;
  extras?: ReactNode;
  onFit?: () => void;
  onActualSize?: () => void;
  fitLabel?: string;
  actualSizeLabel?: string;
  fitPressed?: boolean;
  actualSizePressed?: boolean;
  onZoomOut?: () => void;
  onZoomIn?: () => void;
  onRefresh?: () => void;
  refreshTestId?: string;
}

export function CanvasToolbar({ label = "Graph controls", floating = true, view, extras, onFit, onActualSize, fitLabel = "Fit all", actualSizeLabel = "Reset to 100%", fitPressed, actualSizePressed, onZoomOut, onZoomIn, onRefresh, refreshTestId }: CanvasToolbarProps) {
  return <div
    role="toolbar"
    aria-label={label}
    data-testid="canvas-toolbar"
    data-graph-toolbar
    className={`${floating ? "absolute left-3 top-3" : "relative w-fit"} z-20 flex max-w-[calc(100%-1.5rem)] min-w-0 flex-wrap items-center gap-1.5 rounded-full bg-[var(--ct-bg)] p-1.5 shadow-[var(--ct-shadow-capsule)]`}
    onMouseDown={(event) => event.stopPropagation()}
    onPointerDown={(event) => event.stopPropagation()}
    onTouchStart={(event) => event.stopPropagation()}
    onKeyDown={(event) => event.stopPropagation()}
  >
    {view}
    {onFit && <button type="button" data-testid="canvas-zoom-fit" aria-label={fitLabel} aria-pressed={fitPressed} className="ct-bar-pill" onClick={onFit}>Fit all</button>}
    {onActualSize && <button type="button" data-testid="canvas-zoom-actual" aria-label={actualSizeLabel} aria-pressed={actualSizePressed} className="ct-bar-pill" onClick={onActualSize}>100%</button>}
    {onZoomOut && <button type="button" data-testid="canvas-zoom-out" aria-label="Zoom out" title="Zoom out" className="ct-bar-pill" onClick={onZoomOut}><Minus size={13} aria-hidden="true" /></button>}
    {onZoomIn && <button type="button" data-testid="canvas-zoom-in" aria-label="Zoom in" title="Zoom in" className="ct-bar-pill" onClick={onZoomIn}><Plus size={13} aria-hidden="true" /></button>}
    {onRefresh && <button type="button" data-testid={refreshTestId} aria-label="Refresh" title="Refresh" className="ct-bar-pill" onClick={onRefresh}><RefreshCw size={13} aria-hidden="true" /></button>}
    {extras}
  </div>;
}
