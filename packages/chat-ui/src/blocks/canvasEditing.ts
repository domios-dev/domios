import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent, type RefObject } from "react";
import { DS_CANVAS_ITEM_LIMIT, nextDsCanvasNoteId, type DsCanvas, type DsCanvasEdit } from "@codeterm/chat-core";
import type { ViewportBox } from "@codeterm/canvas-camera";
import { CANVAS_DRAG_THRESHOLD, CANVAS_NOTE_SIZE, CANVAS_NUDGE, noteSpot, resizeFrame, type CanvasLayout, type PlacedCanvasItem } from "./canvasBlockModel";

/** What the owner of a file canvas may do; read-only canvases have none. */
export interface CanvasEditor {
  commit(edit: DsCanvasEdit): boolean;
  undo(): boolean;
  redo(): boolean;
  /** Holds file updates until the returned release runs. */
  hold(): () => void;
  retry(): void;
  failure: string | null;
}

export interface CanvasItemEdit {
  dragging: boolean;
  draft: string | null;
  onBodyPointerDown(event: ReactPointerEvent<HTMLElement>): void;
  onResizePointerDown(event: ReactPointerEvent<HTMLElement>): void;
  onOpenNote(): void;
  onDraft(text: string): void;
  onDraftDone(commit: boolean): void;
}

type Frame = { x: number; y: number; w: number; h: number };
interface Gesture { id: string; mode: "move" | "resize"; startX: number; startY: number; scale: number; start: Frame; keepAspect: boolean; moved: boolean; release: (() => void) | null }
interface Draft { id: string; text: string; release: () => void }

const typing = (target: EventTarget | null) => target instanceof Element && Boolean(target.closest("textarea, input, select, [contenteditable='true']"));

/** The world point at the centre of the visible canvas, read from the camera layer that holds `root`. */
function viewCentre(root: HTMLElement | null, fallback: ViewportBox): { x: number; y: number } {
  const container = root?.closest<HTMLElement>("[aria-label='Zoomable canvas']");
  const layer = root?.parentElement;
  const scale = Number(/matrix\(([^,]+)/.exec(layer ? getComputedStyle(layer).transform : "")?.[1]);
  if (!container || !layer || !Number.isFinite(scale) || scale <= 0) return { x: fallback.x + fallback.width / 2, y: fallback.y + fallback.height / 2 };
  const box = container.getBoundingClientRect(), origin = layer.getBoundingClientRect();
  return { x: (box.left + box.width / 2 - origin.left) / scale, y: (box.top + box.height / 2 - origin.top) / scale };
}

export function useCanvasEditing({ editor, canvas, layout, selected, select, rootRef }: {
  editor: CanvasEditor | undefined;
  canvas: DsCanvas;
  layout: CanvasLayout;
  selected: string | null;
  select: (id: string | null) => void;
  rootRef: RefObject<HTMLElement | null>;
}) {
  const [preview, setPreview] = useState<{ id: string; frame: Frame } | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const latest = useRef({ editor, layout, selected, draft, select });
  latest.current = { editor, layout, selected, draft, select };
  const gesture = useRef<Gesture | null>(null);

  const placedOf = (id: string) => latest.current.layout.items.find((placed) => placed.item.id === id);
  const finishDraft = (commit: boolean) => {
    const current = latest.current.draft;
    if (!current) return;
    // A blur from the unmounting editor must not commit a draft that Escape just cancelled.
    latest.current.draft = null;
    setDraft(null);
    const placed = placedOf(current.id);
    if (commit && placed?.item.kind === "note" && placed.item.markdown !== current.text) latest.current.editor?.commit({ kind: "text", id: current.id, markdown: current.text });
    current.release();
  };
  const openDraft = (id: string, text?: string) => {
    const active = latest.current.editor;
    const placed = placedOf(id);
    if (!active || latest.current.draft) return;
    const markdown = text ?? (placed?.item.kind === "note" ? placed.item.markdown : null);
    if (markdown === null) return;
    const next = { id, text: markdown, release: active.hold() };
    latest.current.draft = next;
    setDraft(next);
  };

  const begin = (event: ReactPointerEvent<HTMLElement>, placed: PlacedCanvasItem, mode: Gesture["mode"]) => {
    if (!latest.current.editor || event.button !== 0 || gesture.current || latest.current.draft?.id === placed.item.id) return;
    event.stopPropagation();
    const rect = event.currentTarget.closest<HTMLElement>("[data-canvas-item]")?.getBoundingClientRect();
    const scale = rect && rect.width > 0 ? rect.width / placed.w : 1;
    gesture.current = { id: placed.item.id, mode, startX: event.clientX, startY: event.clientY, scale, start: { x: placed.x, y: placed.y, w: placed.w, h: placed.h }, keepAspect: placed.item.kind === "image", moved: false, release: null };
    const frameAt = (clientX: number, clientY: number): Frame => {
      const active = gesture.current!;
      const dx = (clientX - active.startX) / active.scale, dy = (clientY - active.startY) / active.scale;
      if (active.mode === "move") return { ...active.start, x: active.start.x + dx, y: active.start.y + dy };
      return { ...active.start, ...resizeFrame(active.start, dx, dy, active.keepAspect) };
    };
    const move = (next: PointerEvent) => {
      const active = gesture.current;
      if (!active) return;
      if (!active.moved && Math.hypot(next.clientX - active.startX, next.clientY - active.startY) < CANVAS_DRAG_THRESHOLD) return;
      if (!active.moved) { active.moved = true; active.release = latest.current.editor?.hold() ?? null; }
      setPreview({ id: active.id, frame: frameAt(next.clientX, next.clientY) });
    };
    const end = (last: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      const active = gesture.current;
      gesture.current = null;
      setPreview(null);
      if (!active?.moved) return;
      const frame = frameAt(last.clientX, last.clientY);
      if (last.type !== "pointercancel") {
        latest.current.editor?.commit({ kind: "frame", id: active.id, frame: active.mode === "move" ? { x: frame.x, y: frame.y } : { w: frame.w, h: frame.h } });
      }
      active.release?.();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  };

  useEffect(() => {
    if (!editor) return;
    const onKey = (event: KeyboardEvent) => {
      const { editor: active, selected: id, draft: open, select: choose } = latest.current;
      if (!active || open || typing(event.target)) return;
      const modifier = event.ctrlKey || event.metaKey;
      const handled = () => { event.preventDefault(); event.stopPropagation(); };
      if (modifier && event.code === "KeyZ") { handled(); if (event.shiftKey) active.redo(); else active.undo(); return; }
      if (modifier && event.code === "KeyY") { handled(); active.redo(); return; }
      if (id === null || modifier || event.altKey) return;
      const placed = placedOf(id);
      if (!placed) return;
      if (event.key === "Delete" || event.key === "Backspace") {
        handled();
        if (active.commit({ kind: "delete", id })) choose(null);
        return;
      }
      if (event.key === "Enter" && placed.item.kind === "note") { handled(); openDraft(id); return; }
      const step = event.shiftKey ? CANVAS_NUDGE.fine : CANVAS_NUDGE.step;
      const nudge: Record<string, [number, number]> = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] };
      const delta = nudge[event.key];
      if (!delta) return;
      handled();
      active.commit({ kind: "frame", id, frame: { x: placed.x + delta[0], y: placed.y + delta[1] } });
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [editor]);

  useEffect(() => () => { latest.current.draft?.release(); }, []);

  const displayLayout = useMemo<CanvasLayout>(() => {
    if (!preview) return layout;
    return { ...layout, items: layout.items.map((placed) => placed.item.id === preview.id ? { ...placed, ...preview.frame } : placed) };
  }, [layout, preview]);

  const itemEdit = (placed: PlacedCanvasItem): CanvasItemEdit | undefined => editor && {
    dragging: preview?.id === placed.item.id,
    draft: draft?.id === placed.item.id ? draft.text : null,
    onBodyPointerDown: (event) => begin(event, placed, "move"),
    onResizePointerDown: (event) => begin(event, placed, "resize"),
    onOpenNote: () => { select(placed.item.id); openDraft(placed.item.id); },
    onDraft: (text) => setDraft((current) => current && { ...current, text }),
    onDraftDone: finishDraft,
  };

  const addNote = () => {
    if (!editor || latest.current.draft) return;
    const id = nextDsCanvasNoteId(canvas);
    const spot = noteSpot(viewCentre(rootRef.current, layout.bounds), layout.items);
    if (!editor.commit({ kind: "insert", index: canvas.items.length, item: { id, kind: "note", markdown: "", ...spot, ...CANVAS_NOTE_SIZE } })) return;
    select(id);
    openDraft(id, "");
  };

  /** Escape closes an open note editor, restoring its text, before it releases the selection. */
  const escape = () => {
    if (!latest.current.draft) return false;
    finishDraft(false);
    return true;
  };

  return { displayLayout, itemEdit, addNote, escape, canAddNote: Boolean(editor) && canvas.items.length < DS_CANVAS_ITEM_LIMIT };
}
