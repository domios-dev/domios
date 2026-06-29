import type { CSSProperties } from "react";
import { BookOpen, LayoutGrid, Map as MapIcon, Maximize2, Minus, Plus, RotateCcw } from "lucide-react";

const bar: CSSProperties = {
  position: "absolute",
  top: 12,
  left: "50%",
  transform: "translateX(-50%)",
  zIndex: 20,
  display: "flex",
  alignItems: "center",
  gap: 6,
  padding: "5px 7px",
  borderRadius: 999,
  background: "color-mix(in srgb, var(--ct-titlebar-bg) 92%, transparent)",
  border: "1px solid var(--ct-border-default)",
  boxShadow: "0 8px 24px -10px rgba(0,0,0,0.55), 0 2px 6px -3px rgba(0,0,0,0.4)",
  backdropFilter: "blur(8px)",
  fontFamily: "var(--ct-font-ui)",
};

const iconBtn = (active = false): CSSProperties => ({
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 5,
  height: 28,
  minWidth: 28,
  padding: "0 8px",
  borderRadius: 999,
  border: "1px solid transparent",
  background: active ? "var(--ct-border-focus)" : "transparent",
  color: active ? "var(--ct-bg)" : "var(--ct-fg)",
  cursor: "pointer",
  fontSize: 12,
  fontWeight: 600,
  lineHeight: 1,
});

const divider: CSSProperties = { width: 1, height: 20, background: "var(--ct-border-subtle)", margin: "0 2px" };

// §24.1: ONE behavior — no Doc/Card/Tile node-mode switch. The toolbar is camera-
// only: zoom / fit / reset + Overview/Read camera presets + minimap. Node content
// adapts to camera zoom (content-LOD), never a manual node mode.
export interface CanvasToolbarProps {
  scale: number;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onFit: () => void;
  onReset: () => void;
  onOverview: () => void;
  onRead: () => void;
  onToggleMinimap: () => void;
  minimapOn: boolean;
}

export function CanvasToolbar({ scale, onZoomIn, onZoomOut, onFit, onReset, onOverview, onRead, onToggleMinimap, minimapOn }: CanvasToolbarProps) {
  return (
    <div data-testid="canvas-toolbar" style={bar} onMouseDown={(e) => e.stopPropagation()}>
      <button type="button" data-testid="canvas-zoom-out" title="Zoom out" style={iconBtn()} onClick={onZoomOut}>
        <Minus size={15} />
      </button>
      <span data-testid="canvas-zoom-level" style={{ fontVariantNumeric: "tabular-nums", fontSize: 11, minWidth: 38, textAlign: "center", opacity: "var(--ct-text-secondary-opacity)" }}>
        {Math.round(scale * 100)}%
      </span>
      <button type="button" data-testid="canvas-zoom-in" title="Zoom in" style={iconBtn()} onClick={onZoomIn}>
        <Plus size={15} />
      </button>
      <button type="button" data-testid="canvas-zoom-fit" title="Fit to view" style={iconBtn()} onClick={onFit}>
        <Maximize2 size={14} />
      </button>
      <button type="button" data-testid="canvas-zoom-reset" title="Reset zoom (100%)" style={iconBtn()} onClick={onReset}>
        <RotateCcw size={14} />
      </button>

      <span style={divider} />

      <button type="button" data-testid="canvas-preset-overview" title="Overview — fit the whole board" style={iconBtn()} onClick={onOverview}>
        <LayoutGrid size={13} /> Overview
      </button>
      <button type="button" data-testid="canvas-preset-read" title="Read — focus the active plan" style={iconBtn()} onClick={onRead}>
        <BookOpen size={13} /> Read
      </button>
      <button type="button" data-testid="canvas-minimap-toggle" title="Toggle minimap" style={iconBtn(minimapOn)} onClick={onToggleMinimap}>
        <MapIcon size={14} />
      </button>
    </div>
  );
}
