import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { CanvasNotice } from "./CanvasNotice";
import { parseDsCanvas, type DsCanvas } from "@codeterm/chat-core";
import { readThemeVars } from "../HtmlPreviewBlock";
import { CanvasItemView } from "./canvasItems";
import { CanvasViewer } from "./CanvasViewer";
import type { CanvasEditor } from "./canvasEditing";
import { useVisibleIds } from "./useVisibleIds";
import { CANVAS_INLINE_HEIGHT, canvasViewTarget, chooseLiveFrames, fitPreview, isLiveFrameItem, layoutCanvas } from "./canvasBlockModel";

const DOTTED_GROUND = {
  backgroundColor: "var(--ct-bg)",
  backgroundImage: "radial-gradient(circle, color-mix(in srgb, var(--ct-accent) 25%, transparent) 1px, transparent 1px)",
  backgroundSize: "24px 24px",
};

/** A `<ds-canvas>` body: a static preview in the chat that opens the full canvas. */
export function CanvasBlock({ body }: { body: string }) {
  const parsed = useMemo(() => parseDsCanvas(body), [body]);
  if (!parsed.ok) return <CanvasNotice text="This canvas could not be shown because its description is not valid." reason={parsed.reason} />;
  return <CanvasPreview canvas={parsed.canvas} />;
}


function usePreviewWidth() {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const measure = () => setWidth(element.clientWidth || element.getBoundingClientRect().width);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return { ref, width };
}

/** The static preview; a file canvas adds its source path, a named view and, for its owner, an editor. */
export function CanvasPreview({ canvas, view, editor, fullscreenRequest = 0, details }: { canvas: DsCanvas; view?: string; editor?: CanvasEditor; fullscreenRequest?: number; details?: string }) {
  const layout = useMemo(() => layoutCanvas(canvas), [canvas]);
  const theme = useMemo(() => readThemeVars(), []);
  const [open, setOpen] = useState(false);
  useEffect(() => { if (fullscreenRequest) setOpen(true); }, [fullscreenRequest]);
  const { ref, width } = usePreviewWidth();
  const { observe, isVisible } = useVisibleIds();
  const observePreview = observe("preview");
  const previewRef = useCallback((element: HTMLDivElement | null) => { ref.current = element; observePreview(element); }, [ref, observePreview]);
  const fit = fitPreview(canvasViewTarget(canvas, view)?.bounds ?? layout.bounds, width, CANVAS_INLINE_HEIGHT);
  const onScreen = !open && width > 0 && isVisible("preview");
  const live = chooseLiveFrames(layout.items.filter((placed) => isLiveFrameItem(placed.item)).map((placed) => ({ id: placed.item.id, visible: onScreen })), null);
  const title = canvas.title || "Canvas";
  const count = `${layout.items.length} ${layout.items.length === 1 ? "item" : "items"}`;
  return (
    <figure data-testid="chat-ds-canvas" className="my-2 min-w-0 max-w-full">
      <div ref={previewRef} role="button" tabIndex={0} aria-label={`Open canvas ${title}, ${count}`} aria-haspopup="dialog"
        className="relative w-full cursor-zoom-in overflow-hidden focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]"
        style={{ height: CANVAS_INLINE_HEIGHT, border: "1px solid var(--ct-border)", borderRadius: 8, boxSizing: "border-box", ...DOTTED_GROUND }}
        onClick={() => setOpen(true)}
        onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); setOpen(true); } }}>
        {fit.scale > 0 && <div aria-hidden data-canvas-preview-layer="" style={{ position: "absolute", left: 0, top: 0, transformOrigin: "0 0", transform: `translate(${fit.x}px, ${fit.y}px) scale(${fit.scale})`, pointerEvents: "none" }}>
          {layout.items.map((placed) => <CanvasItemView key={placed.item.id} placed={placed} live={live.has(placed.item.id)} interactive={false} theme={theme} />)}
        </div>}
      </div>
      <figcaption className="mt-1 flex min-w-0 gap-1.5 text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
        <span className="min-w-0 truncate" style={{ color: "var(--ct-fg)" }}>{title}</span>
        <span aria-hidden>·</span>
        <span className="shrink-0">{count}</span>
        {details && <details data-canvas-details="" className="min-w-0 opacity-80"><summary className="cursor-pointer select-none">Details</summary><p className="m-0 mt-1 break-words">{details}</p></details>}
      </figcaption>
      {open && <CanvasViewer canvas={canvas} layout={layout} title={title} view={view} editor={editor} anchorRef={ref} onClose={() => setOpen(false)} />}
    </figure>
  );
}
