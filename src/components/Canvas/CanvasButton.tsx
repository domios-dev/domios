import { LayoutGrid } from "lucide-react";
import { canvasFocusStore } from "./canvasFocusStore";
import { UI_TRAY_BTN } from "../shared/design";

export interface CanvasButtonProps {
  active: boolean;
}

export function CanvasButton({ active }: CanvasButtonProps) {
  return (
    <button
      type="button"
      data-testid="canvas-toolbar-button"
      onClick={() => canvasFocusStore.toggleWorkspace(active)}
      aria-label={active ? "Close workspace canvas" : "Open workspace canvas"}
      title={active ? "Close workspace canvas" : "Open workspace canvas"}
      className={`${UI_TRAY_BTN} ${active ? "text-[var(--ct-accent)]" : ""}`}
      aria-pressed={active}
    >
      <LayoutGrid size={14} />
    </button>
  );
}
