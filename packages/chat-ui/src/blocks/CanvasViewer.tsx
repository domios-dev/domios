import { useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import { LayoutDashboard, StickyNote, X } from "lucide-react";
import type { DsCanvas } from "@codeterm/chat-core";
import { CanvasHeader } from "../CanvasHeader";
import { CANVAS_GHOST_BUTTON } from "../CanvasToolbar";
import { FullscreenOverlay } from "../FullscreenOverlay";
import { ZoomCanvas } from "../ZoomCanvas";
import { readThemeVars } from "../HtmlPreviewBlock";
import { CanvasNoticeBody } from "./CanvasNotice";
import { CanvasItemView } from "./canvasItems";
import { useCanvasEditing, type CanvasEditor, type CanvasItemEdit } from "./canvasEditing";
import { useVisibleIds } from "./useVisibleIds";
import { canvasPagerLabel, canvasPagerTargets, canvasViewTarget, chooseLiveFrames, isLiveFrameItem, reconcilePager, stepCanvasPager, type CanvasLayout, type CanvasPagerPosition, type PlacedCanvasItem } from "./canvasBlockModel";

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

function CanvasStageItems({ layout, selected, onSelect, release, root, itemEdit }: {
  layout: CanvasLayout;
  selected: string | null;
  onSelect: (id: string) => void;
  release: () => void;
  root: RefObject<HTMLDivElement | null>;
  itemEdit: (placed: PlacedCanvasItem) => CanvasItemEdit | undefined;
}) {
  const theme = useMemo(() => readThemeVars(), []);
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
        <CanvasItemView key={placed.item.id} itemRef={observe(placed.item.id)} placed={placed} live={live.has(placed.item.id)} selected={selected === placed.item.id} interactive theme={theme} onSelect={onSelect} edit={itemEdit(placed)} />
      ))}
    </div>
  );
}

/** Overlays the top of the stage on the header's surface, so the board and camera never shift. */
function SaveFailure({ reason, onRetry }: { reason: string; onRetry: () => void }) {
  return (
    <div role="status" data-canvas-save-failure="" className="absolute inset-x-0 top-full z-10 border-b border-[var(--ct-border)] px-3 py-1 text-xs" style={{ color: "var(--ct-muted-fg)", background: "var(--ct-bg)" }}>
      <CanvasNoticeBody text="Couldn't save this canvas." reason={reason} action={<button type="button" className={CANVAS_GHOST_BUTTON} onClick={onRetry}>Try again</button>} />
    </div>
  );
}

export function CanvasViewer({ canvas, layout, title, view, editor, anchorRef, onClose }: {
  canvas: DsCanvas;
  layout: CanvasLayout;
  title: string;
  /** A named view the viewer opens on. */
  view?: string;
  /** Present only for the owner of a local canvas file. */
  editor?: CanvasEditor;
  anchorRef: RefObject<HTMLElement | null>;
  onClose: () => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const targets = useMemo(() => canvasPagerTargets(canvas, layout), [canvas, layout]);
  const [initial] = useState(() => canvasViewTarget(canvas, view));
  const [position, setPosition] = useState<CanvasPagerPosition | null>(initial ? { key: `view:${view}`, index: initial.index } : null);
  const active = reconcilePager(targets, position);
  const activeKey = active === null ? null : targets[active].key;
  // A clamped pager adopts its new target, so later reloads follow that one.
  useLayoutEffect(() => {
    if (active !== null && activeKey !== null && activeKey !== position?.key) setPosition({ key: activeKey, index: active });
  }, [active, activeKey, position?.key]);
  const [focus, setFocus] = useState<{ request: number; bounds: CanvasLayout["bounds"] } | null>(initial ? { request: 1, bounds: initial.bounds } : null);
  const [selected, setSelected] = useState<string | null>(null);
  const editing = useCanvasEditing({ editor, canvas, layout, selected, select: setSelected, rootRef: stageRef });
  const navigate = (direction: number) => {
    const next = stepCanvasPager(active, targets.length, direction);
    setPosition(next === null ? null : { key: targets[next].key, index: next });
    setFocus((previous) => ({ request: (previous?.request ?? 0) + 1, bounds: next === null ? layout.bounds : targets[next].bounds }));
  };
  const release = () => {
    if (editing.escape()) return;
    setSelected(null);
    panelRef.current?.querySelector<HTMLElement>('[aria-label="Zoomable canvas"]')?.focus({ preventScroll: true });
  };
  const select = (index: number) => setSelected(editing.displayLayout.items[index]?.item.id ?? null);
  const unit = canvas.views.length ? "views" : "items";
  return (
    <FullscreenOverlay panelRef={panelRef} closeButton={false} label={`Canvas ${title}`} onClose={onClose} returnFocusRef={anchorRef}>
      <div className="-m-2 flex min-h-0 flex-1 flex-col" data-testid="chat-canvas-viewer">
        <ZoomCanvas bounds={layout.bounds} focus={focus} onNavigate={targets.length > 1 ? navigate : undefined} onSelect={select} toolbar="header" followContent={!editor}
          renderHeader={(controls) => <div className="relative">
            <CanvasHeader
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
              view={<>
                {editing.canAddNote && <button type="button" className={CANVAS_GHOST_BUTTON} aria-label="Add note" title="Add note" onClick={editing.addNote}><StickyNote size={13} aria-hidden /></button>}
                {controls}
              </>}
              close={<button type="button" className={CANVAS_GHOST_BUTTON} onClick={onClose} aria-label="Close canvas"><X size={13} aria-hidden /></button>}
            />
            {editor?.failure && <SaveFailure reason={editor.failure} onRetry={editor.retry} />}
          </div>}>
          <CanvasStageItems layout={editing.displayLayout} selected={selected} onSelect={setSelected} release={release} root={stageRef} itemEdit={editing.itemEdit} />
        </ZoomCanvas>
      </div>
    </FullscreenOverlay>
  );
}
