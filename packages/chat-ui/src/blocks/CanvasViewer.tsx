import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { LayoutDashboard, X } from "lucide-react";
import type { DsCanvas } from "@codeterm/chat-core";
import { CanvasHeader } from "../CanvasHeader";
import { CANVAS_GHOST_BUTTON } from "../CanvasToolbar";
import { FullscreenOverlay } from "../FullscreenOverlay";
import { ZoomCanvas } from "../ZoomCanvas";
import { readThemeVars } from "../HtmlPreviewBlock";
import { CanvasItemView } from "./canvasItems";
import { useVisibleIds } from "./useVisibleIds";
import { canvasPagerLabel, canvasPagerTargets, chooseLiveFrames, isLiveFrameItem, stepCanvasPager, type CanvasLayout } from "./canvasBlockModel";

/** Escape inside a selected item returns it to the canvas before the viewer may close. */
function useSelectionEscape(selected: RefObject<string | null>, root: RefObject<HTMLDivElement | null>, release: () => void) {
  const latest = useRef(release);
  latest.current = release;
  useEffect(() => {
    const handled = (event: Event) => {
      event.stopImmediatePropagation();
      event.preventDefault();
      latest.current();
    };
    const onKey = (event: KeyboardEvent) => {
      if (selected.current !== null && (event.code === "Escape" || event.key === "Escape")) handled(event);
    };
    const onFrame = (event: MessageEvent) => {
      if (selected.current === null || event.data?.source !== "ct-html-preview" || event.data?.action !== "close" || event.source === null) return;
      if (Array.from(root.current?.querySelectorAll("iframe") ?? []).some((frame) => frame.contentWindow === event.source)) handled(event);
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("message", onFrame);
    return () => { window.removeEventListener("keydown", onKey, true); window.removeEventListener("message", onFrame); };
  }, [selected, root]);
}

function CanvasStageItems({ layout, selected, onSelect, release }: { layout: CanvasLayout; selected: string | null; onSelect: (id: string) => void; release: () => void }) {
  const theme = useMemo(() => readThemeVars(), []);
  const root = useRef<HTMLDivElement>(null);
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  useSelectionEscape(selectedRef, root, release);
  const { observe, isVisible } = useVisibleIds();
  const frames = layout.items.filter((placed) => isLiveFrameItem(placed.item)).map((placed) => ({ id: placed.item.id, visible: isVisible(placed.item.id) }));
  const live = chooseLiveFrames(frames, selected);
  return (
    <div ref={root} data-canvas-stage="">
      <div aria-hidden onClick={release} style={{ position: "absolute", left: layout.bounds.x - 4000, top: layout.bounds.y - 4000, width: layout.bounds.width + 8000, height: layout.bounds.height + 8000 }} />
      {layout.items.map((placed) => (
        <CanvasItemView key={placed.item.id} itemRef={observe(placed.item.id)} placed={placed} live={live.has(placed.item.id)} selected={selected === placed.item.id} interactive theme={theme} onSelect={onSelect} />
      ))}
    </div>
  );
}

export function CanvasViewer({ canvas, layout, title, anchorRef, onClose }: {
  canvas: DsCanvas;
  layout: CanvasLayout;
  title: string;
  anchorRef: RefObject<HTMLElement | null>;
  onClose: () => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  const targets = useMemo(() => canvasPagerTargets(canvas, layout), [canvas, layout]);
  const [active, setActive] = useState<number | null>(null);
  const [focus, setFocus] = useState<{ request: number; bounds: CanvasLayout["bounds"] } | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const navigate = (direction: number) => {
    const next = stepCanvasPager(active, targets.length, direction);
    setActive(next);
    setFocus((previous) => ({ request: (previous?.request ?? 0) + 1, bounds: next === null ? layout.bounds : targets[next].bounds }));
  };
  const release = () => {
    setSelected(null);
    panelRef.current?.querySelector<HTMLElement>('[aria-label="Zoomable canvas"]')?.focus({ preventScroll: true });
  };
  const select = (index: number) => setSelected(layout.items[index]?.item.id ?? null);
  const unit = canvas.views.length ? "views" : "items";
  return (
    <FullscreenOverlay panelRef={panelRef} closeButton={false} label={`Canvas ${title}`} onClose={onClose} returnFocusRef={anchorRef}>
      <div className="-m-2 flex min-h-0 flex-1 flex-col" data-testid="chat-canvas-viewer">
        <ZoomCanvas bounds={layout.bounds} focus={focus} onNavigate={targets.length > 1 ? navigate : undefined} onSelect={select} toolbar="header"
          renderHeader={(controls) => <CanvasHeader
            title={<><LayoutDashboard size={14} aria-hidden className="shrink-0 @max-[560px]:hidden" /><span className="min-w-0 truncate font-medium text-[var(--ct-fg)]">{title}</span></>}
            navigator={targets.length > 1 && <nav aria-label={`Canvas ${unit}`} className="flex shrink-0 items-center gap-0.5 @max-[560px]:gap-0">
              <button type="button" className={CANVAS_GHOST_BUTTON} aria-label={`Previous ${unit === "views" ? "view" : "item"}`} onClick={() => navigate(-1)}>‹</button>
              <span aria-live="polite" title={active === null ? `All ${unit}` : targets[active].label} className="flex min-w-0 max-w-[24ch] items-baseline gap-1.5 @max-[560px]:max-w-[12ch]">
                {active === null ? canvasPagerLabel(active, targets.length) : <>
                  <span className="min-w-0 truncate text-[var(--ct-fg)]">{targets[active].label}</span>{" "}
                  <span className="shrink-0 tabular-nums text-[var(--ct-muted-fg)]">{canvasPagerLabel(active, targets.length)}</span>
                </>}
              </span>
              <button type="button" className={CANVAS_GHOST_BUTTON} aria-label={`Next ${unit === "views" ? "view" : "item"}`} onClick={() => navigate(1)}>›</button>
            </nav>}
            view={controls}
            close={<button type="button" className={CANVAS_GHOST_BUTTON} onClick={onClose} aria-label="Close canvas"><X size={13} aria-hidden /></button>}
          />}>
          <CanvasStageItems layout={layout} selected={selected} onSelect={setSelected} release={release} />
        </ZoomCanvas>
      </div>
    </FullscreenOverlay>
  );
}
