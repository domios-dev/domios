import type { ReactNode } from "react";
import { Minus, Plus, RefreshCw } from "lucide-react";

export interface CanvasToolbarProps {
  label?: string;
  view?: ReactNode;
  extras?: ReactNode;
  onFit?: () => void;
  onZoomOut?: () => void;
  onZoomIn?: () => void;
  onRefresh?: () => void;
  refreshTestId?: string;
}

export function CanvasToolbar({ label = "Graph controls", view, extras, onFit, onZoomOut, onZoomIn, onRefresh, refreshTestId }: CanvasToolbarProps) {
  return <div
    role="toolbar"
    aria-label={label}
    data-testid="canvas-toolbar"
    data-graph-toolbar
    className="absolute left-3 top-3 z-20 flex max-w-[calc(100%-1.5rem)] min-w-0 flex-wrap items-center gap-1.5 rounded-full bg-[var(--ct-bg)] p-1.5 shadow-[var(--ct-shadow-capsule)]"
    onMouseDown={(event) => event.stopPropagation()}
    onPointerDown={(event) => event.stopPropagation()}
    onTouchStart={(event) => event.stopPropagation()}
    onKeyDown={(event) => event.stopPropagation()}
  >
    {view}
    {onFit && <button type="button" data-testid="canvas-zoom-fit" aria-label="Fit all" className="ct-bar-pill" onClick={onFit}>Fit all</button>}
    {onZoomOut && <button type="button" data-testid="canvas-zoom-out" aria-label="Zoom out" title="Zoom out" className="ct-bar-pill" onClick={onZoomOut}><Minus size={13} aria-hidden="true" /></button>}
    {onZoomIn && <button type="button" data-testid="canvas-zoom-in" aria-label="Zoom in" title="Zoom in" className="ct-bar-pill" onClick={onZoomIn}><Plus size={13} aria-hidden="true" /></button>}
    {onRefresh && <button type="button" data-testid={refreshTestId} aria-label="Refresh" title="Refresh" className="ct-bar-pill" onClick={onRefresh}><RefreshCw size={13} aria-hidden="true" /></button>}
    {extras}
  </div>;
}
