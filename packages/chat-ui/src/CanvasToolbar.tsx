import type { ReactNode } from "react";
import { Minus, Plus, RefreshCw } from "lucide-react";

export const CANVAS_GHOST_BUTTON = "inline-flex h-8 min-w-8 shrink-0 items-center justify-center gap-1.5 rounded-md border-0 bg-transparent px-2 text-xs text-[var(--ct-muted-fg)] hover:bg-[var(--ct-surface)] hover:text-[var(--ct-fg)] focus-visible:bg-[var(--ct-surface)] focus-visible:text-[var(--ct-fg)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)] aria-pressed:bg-[var(--ct-surface)] aria-pressed:text-[var(--ct-fg)] [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11";
export const CANVAS_SEPARATOR = "mx-1.5 h-[18px] w-px shrink-0 bg-[var(--ct-border)] @max-[560px]:hidden";

export interface CanvasToolbarProps {
  appearance?: "floating" | "inline";
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

export function CanvasToolbar({ appearance = "floating", label = "Graph controls", floating = true, view, extras, onFit, onActualSize, fitLabel = "Fit all", actualSizeLabel = "Reset to 100%", fitPressed, actualSizePressed, onZoomOut, onZoomIn, onRefresh, refreshTestId }: CanvasToolbarProps) {
  const inline = appearance === "inline";
  const button = inline ? CANVAS_GHOST_BUTTON : "ct-bar-pill";
  return <div
    role="toolbar"
    aria-label={label}
    data-testid="canvas-toolbar"
    data-graph-toolbar
    className={inline ? "flex shrink-0 items-center gap-0.5 @max-[560px]:gap-0" : `${floating ? "absolute left-3 top-3" : "relative w-fit"} z-20 flex max-w-[calc(100%-1.5rem)] min-w-0 flex-wrap items-center gap-1.5 rounded-full bg-[var(--ct-bg)] p-1.5 shadow-[var(--ct-shadow-capsule)]`}
    onMouseDown={(event) => event.stopPropagation()}
    onPointerDown={(event) => event.stopPropagation()}
    onTouchStart={(event) => event.stopPropagation()}
    onKeyDown={(event) => event.stopPropagation()}
  >
    {view}
    {onFit && <button type="button" data-testid="canvas-zoom-fit" aria-label={fitLabel} aria-pressed={fitPressed} className={button} onClick={onFit}>{inline ? "Fit" : "Fit all"}</button>}
    {inline && onFit && <span aria-hidden className={CANVAS_SEPARATOR} />}
    {!inline && onActualSize && <button type="button" data-testid="canvas-zoom-actual" aria-label={actualSizeLabel} aria-pressed={actualSizePressed} className={button} onClick={onActualSize}>100%</button>}
    {onZoomOut && <button type="button" data-testid="canvas-zoom-out" aria-label="Zoom out" title="Zoom out" className={inline ? `${button} @max-[560px]:hidden` : button} onClick={onZoomOut}><Minus size={13} aria-hidden="true" /></button>}
    {inline && onActualSize && <button type="button" data-testid="canvas-zoom-actual" aria-label={actualSizeLabel} title={actualSizeLabel} aria-pressed={actualSizePressed} className={`${button} tabular-nums [@media(pointer:coarse)]:hidden`} onClick={onActualSize}>{extras}</button>}
    {onZoomIn && <button type="button" data-testid="canvas-zoom-in" aria-label="Zoom in" title="Zoom in" className={inline ? `${button} @max-[560px]:hidden` : button} onClick={onZoomIn}><Plus size={13} aria-hidden="true" /></button>}
    {onRefresh && <button type="button" data-testid={refreshTestId} aria-label="Refresh" title="Refresh" className={button} onClick={onRefresh}><RefreshCw size={13} aria-hidden="true" /></button>}
    {!inline && extras}
  </div>;
}
