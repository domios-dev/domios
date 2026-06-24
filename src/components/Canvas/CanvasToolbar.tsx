import type { CSSProperties } from "react";
import { Eye, EyeOff, FileText, LayoutGrid, Maximize2, Minus, Plus, RotateCcw, Square } from "lucide-react";
import type { CanvasViewModeSetting } from "./cards";

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

const MODES: { key: CanvasViewModeSetting; label: string; Icon: typeof Square; title: string }[] = [
  { key: "document", label: "Doc", Icon: FileText, title: "Document — full markdown body" },
  { key: "card", label: "Card", Icon: LayoutGrid, title: "Card — title + digest" },
  { key: "tile", label: "Tile", Icon: Square, title: "Tile — title + status only" },
];

export interface CanvasToolbarProps {
  scale: number;
  viewModeSetting: CanvasViewModeSetting;
  showText: boolean;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onFit: () => void;
  onReset: () => void;
  onSetMode: (mode: CanvasViewModeSetting) => void;
  onToggleText: () => void;
}

export function CanvasToolbar({
  scale,
  viewModeSetting,
  showText,
  onZoomIn,
  onZoomOut,
  onFit,
  onReset,
  onSetMode,
  onToggleText,
}: CanvasToolbarProps) {
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

      {MODES.map(({ key, label, Icon, title }) => {
        const active = viewModeSetting === key;
        return (
          <button
            key={key}
            type="button"
            data-testid={`canvas-mode-${key}`}
            data-active={active ? "true" : "false"}
            title={title}
            style={iconBtn(active)}
            onClick={() => onSetMode(key)}
          >
            <Icon size={13} />
            {label}
          </button>
        );
      })}

      <span style={divider} />

      <button
        type="button"
        data-testid="canvas-text-toggle"
        data-active={showText ? "true" : "false"}
        title={showText ? "Hide bodies (declutter)" : "Show bodies"}
        style={iconBtn(!showText)}
        onClick={onToggleText}
      >
        {showText ? <Eye size={14} /> : <EyeOff size={14} />}
      </button>
    </div>
  );
}
