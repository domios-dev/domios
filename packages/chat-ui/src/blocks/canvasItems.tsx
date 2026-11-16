import { useContext, useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import type { DsCanvasItem } from "@codeterm/chat-core";
import { buildHtmlPreviewDocument, HTML_PREVIEW_MESSAGE_SOURCE, sanitizeLocalHtml, SandboxedHtmlFrame, type ThemeVars } from "../HtmlPreviewBlock";
import { CameraRestContext, sharpFrameZoom } from "../canvasRest";
import { loadLocalArtifact } from "../localArtifactLoader";
import { loadHtmlFile } from "../htmlFileLoader";
import { useLocalArtifactTabId } from "../LocalArtifactScope";
import { RichMarkdown } from "../MarkdownBody";
import { canvasItemLabel, isLiveFrameItem, type PlacedCanvasItem } from "./canvasBlockModel";
import { CanvasSourceContext } from "./canvasFilePort";
import type { CanvasItemEdit } from "./canvasEditing";

type FileContent = { kind: "image"; src: string } | { kind: "text"; text: string; markdown: boolean } | { kind: "html"; html: string };
type FileState = { status: "loading" } | { status: "ready"; content: FileContent } | { status: "error"; reason: string };

const isMarkdownPath = (path: string) => /\.(md|markdown)$/i.test(path);
const FAILURE_TEXT: Record<DsCanvasItem["kind"], string> = {
  image: "This image is not available.",
  page: "This page is not available.",
  document: "This document is not available.",
  html: "This HTML is not available.",
  note: "This note is not available.",
};

async function readItemFile(kind: DsCanvasItem["kind"], src: string, tabId: string | undefined, canvas: string | undefined): Promise<FileContent> {
  if (kind === "image") {
    const file = await loadLocalArtifact(src, "image", tabId, canvas);
    if (!file.data.startsWith("data:image/")) throw new Error("The file is not an image.");
    return { kind: "image", src: file.data };
  }
  if (kind === "page" && !isMarkdownPath(src)) return { kind: "html", html: sanitizeLocalHtml(await loadHtmlFile(src, tabId, canvas)).html };
  const file = await loadLocalArtifact(src, "document", tabId, canvas);
  return { kind: "text", text: file.data, markdown: file.media_type === "text/markdown" };
}

/** Keyed by the file, not the item object, so moving or resizing an item never reloads it. */
function useItemFile(item: DsCanvasItem): FileState | null {
  const tabId = useLocalArtifactTabId();
  const canvas = useContext(CanvasSourceContext);
  const src = "src" in item ? item.src : null;
  const kind = item.kind;
  const [state, setState] = useState<{ key: string; value: FileState } | null>(null);
  const key = `${kind}:${src}:${tabId ?? ""}:${canvas ?? ""}`;
  useEffect(() => {
    if (src === null) return;
    let current = true;
    readItemFile(kind, src, tabId, canvas).then(
      (content) => { if (current) setState({ key, value: { status: "ready", content } }); },
      (error: unknown) => { if (current) setState({ key, value: { status: "error", reason: error instanceof Error ? error.message : String(error) } }); },
    );
    return () => { current = false; };
  }, [key, kind, src, tabId, canvas]);
  if (src === null) return null;
  return state?.key === key ? state.value : { status: "loading" };
}

/** Lets a selected scroll box scroll instead of zooming the canvas behind it. */
function useOwnWheel(enabled: boolean) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const element = ref.current;
    if (!enabled || !element) return;
    const keep = (event: WheelEvent) => event.stopPropagation();
    element.addEventListener("wheel", keep, { passive: true });
    return () => element.removeEventListener("wheel", keep);
  }, [enabled]);
  return ref;
}

export function CanvasItemFailure({ text, reason }: { text: string; reason?: string }) {
  return (
    <div role="note" data-canvas-failure="" className="flex h-full w-full flex-col items-center justify-center gap-1 border border-[var(--ct-border)] p-4 text-center text-xs" style={{ color: "var(--ct-muted-fg)", background: "var(--ct-bg)" }}>
      <p className="m-0">{text}</p>
      {reason && <details className="relative z-[1] max-w-full text-[11px] opacity-80"><summary className="cursor-pointer select-none">Details</summary><p className="m-0 mt-1 break-words">{reason}</p></details>}
    </div>
  );
}

function FramePlaceholder({ label }: { label: string }) {
  return <div data-canvas-frame-idle="" className="flex h-full w-full items-center justify-center border border-[var(--ct-border)] p-4 text-xs" style={{ color: "var(--ct-muted-fg)", background: "var(--ct-bg)" }}><span className="truncate">{label}</span></div>;
}

/** At rest the frame is counter-scaled and its content zoomed, so the engine rasterizes it at screen size instead of stretching it. */
function CanvasFrame({ doc, label, active }: { doc: string; label: string; active: boolean }) {
  const zoom = sharpFrameZoom(useContext(CameraRestContext));
  const ref = useRef<HTMLIFrameElement>(null);
  useEffect(() => {
    const frame = ref.current;
    if (!frame) return;
    const send = () => frame.contentWindow?.postMessage({ source: HTML_PREVIEW_MESSAGE_SOURCE, zoom }, "*");
    send();
    frame.addEventListener("load", send);
    return () => frame.removeEventListener("load", send);
  }, [zoom, doc]);
  const sharp: CSSProperties = zoom === 1 ? {} : { width: `${zoom * 100}%`, height: `${zoom * 100}%`, transform: `scale(${1 / zoom})`, transformOrigin: "0 0" };
  const style: CSSProperties = { display: "block", width: "100%", height: "100%", border: "1px solid var(--ct-border)", boxSizing: "border-box", background: "var(--ct-bg)", pointerEvents: active ? "auto" : "none", ...sharp };
  return <SandboxedHtmlFrame ref={ref} doc={doc} title={label} testId="chat-canvas-frame" style={style} />;
}

function Markdown({ text }: { text: string }) {
  return <RichMarkdown text={text} localArtifacts={false} interactivePreviews={false} />;
}

function TextBox({ children, scrolls, fill }: { children: ReactNode; scrolls: boolean; fill: boolean }) {
  const ref = useOwnWheel(scrolls);
  return (
    <div ref={ref} className={`h-full w-full p-3 text-xs ${scrolls ? "overflow-auto" : "overflow-hidden"} ${fill ? "rounded-md" : "border border-[var(--ct-border)]"}`}
      style={{ background: fill ? "var(--ct-surface)" : "var(--ct-bg)", color: "var(--ct-fg)", pointerEvents: scrolls ? "auto" : "none" }}>
      {children}
    </div>
  );
}

function ItemContent({ item, live, active, theme }: { item: DsCanvasItem; live: boolean; active: boolean; theme: ThemeVars }) {
  const file = useItemFile(item);
  const label = canvasItemLabel(item);
  const html = item.kind === "html" ? item.html : null;
  const inlineDoc = useMemo(() => html === null ? null : buildHtmlPreviewDocument(html, theme), [html, theme]);
  const pageDoc = useMemo(() => file?.status === "ready" && file.content.kind === "html" ? buildHtmlPreviewDocument(file.content.html, theme) : null, [file, theme]);
  if (item.kind === "note") return <TextBox scrolls={active} fill><Markdown text={item.markdown} /></TextBox>;
  if (file?.status === "error") return <CanvasItemFailure text={FAILURE_TEXT[item.kind]} reason={file.reason} />;
  if (file?.status === "loading") return <div role="status" aria-label={`Loading ${label}`} className="h-full w-full" />;
  const doc = inlineDoc ?? pageDoc;
  if (doc !== null) return live ? <CanvasFrame doc={doc} label={label} active={active} /> : <FramePlaceholder label={label} />;
  const content = file?.status === "ready" ? file.content : null;
  if (content?.kind === "image") return <ImageContent src={content.src} label={label} />;
  if (content?.kind === "text") return <TextBox scrolls={active} fill={false}>{content.markdown ? <Markdown text={content.text} /> : <pre className="m-0 whitespace-pre-wrap break-words">{content.text}</pre>}</TextBox>;
  return <CanvasItemFailure text={FAILURE_TEXT[item.kind]} />;
}

function ImageContent({ src, label }: { src: string; label: string }) {
  const [failed, setFailed] = useState(false);
  if (failed) return <CanvasItemFailure text={FAILURE_TEXT.image} reason="The image data could not be decoded." />;
  return <img src={src} alt={label} draggable={false} onError={() => setFailed(true)} className="block h-full w-full object-contain" />;
}

const ITEM_RING = "outline-offset-[3px]";

/** One placed item; the viewer adds hover and selection, the inline preview never takes input. */
/** A note in edit mode: its Markdown source on the same soft surface, with no extra border. */
function NoteEditor({ text, edit }: { text: string; edit: CanvasItemEdit }) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { ref.current?.focus({ preventScroll: true }); }, []);
  const stop = (event: { stopPropagation(): void }) => event.stopPropagation();
  return (
    <textarea ref={ref} value={text} aria-label="Note text" data-canvas-note-editor=""
      className="block h-full w-full resize-none rounded-md border-0 p-3 text-xs outline-none"
      style={{ background: "var(--ct-surface)", color: "var(--ct-fg)", fontFamily: "inherit", letterSpacing: "inherit", margin: 0 }}
      onChange={(event) => edit.onDraft(event.target.value)}
      onBlur={() => edit.onDraftDone(true)}
      onKeyDown={(event) => {
        if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); edit.onDraftDone(true); }
      }}
      onPointerDown={stop} onMouseDown={stop} onDoubleClick={stop} onWheel={stop} />
  );
}

/** World distance from the item edge to the centre line of its selection ring (3px offset + half of 2px). */
const RING_CENTRE = 4;
const coarsePointer = () => typeof matchMedia === "function" && matchMedia("(pointer: coarse)").matches;

/**
 * One 8px square centred on the selection ring's bottom-right corner, with a 16px hit area (24px
 * for coarse pointers); counter-scaled so it keeps its screen size at every zoom.
 */
function ResizeHandle({ edit }: { edit: CanvasItemEdit }) {
  const rest = useContext(CameraRestContext);
  const lastScale = useRef(1);
  if (rest !== null && rest > 0) lastScale.current = rest;
  const inverse = 1 / lastScale.current;
  const hit = (coarsePointer() ? 24 : 16) * inverse;
  const square = 8 * inverse;
  const offset = -(RING_CENTRE + hit / 2);
  return (
    <div aria-hidden data-canvas-resize="" onPointerDown={edit.onResizePointerDown} onMouseDown={(event) => event.stopPropagation()} onClick={(event) => event.stopPropagation()}
      className="absolute flex items-center justify-center" style={{ right: offset, bottom: offset, width: hit, height: hit, cursor: "nwse-resize", touchAction: "none" }}>
      <span className="block" style={{ width: square, height: square, background: "var(--ct-accent)" }} />
    </div>
  );
}

export function CanvasItemView({ placed, live, selected = false, interactive, theme, onSelect, itemRef, edit }: {
  itemRef?: (element: HTMLDivElement | null) => void;
  placed: PlacedCanvasItem;
  live: boolean;
  selected?: boolean;
  interactive: boolean;
  theme: ThemeVars;
  onSelect?: (id: string) => void;
  /** Owner editing; html and page frames then stay non-interactive so a drag always moves. */
  edit?: CanvasItemEdit;
}) {
  const { item } = placed;
  const label = canvasItemLabel(item);
  const takesInput = interactive && selected && !(edit && isLiveFrameItem(item));
  const outline = selected ? { outline: "2px solid var(--ct-accent)" } : {};
  const editing = edit?.draft !== null && edit?.draft !== undefined;
  const editStyle: CSSProperties = edit ? { cursor: edit.dragging ? "grabbing" : "grab", userSelect: "none", touchAction: "none" } : {};
  return (
    <div ref={itemRef} data-canvas-item={item.id} data-zoom-target={interactive ? placed.index : undefined} data-selected={selected || undefined}
      role={interactive ? "group" : undefined} aria-label={interactive ? label : undefined}
      className={`${ITEM_RING} ${interactive && !selected ? "hover:outline hover:outline-1 hover:outline-[var(--ct-border)]" : ""}`}
      style={{ position: "absolute", left: placed.x, top: placed.y, width: placed.w, height: placed.h, ...outline, ...editStyle }}
      onPointerDown={edit && !editing ? edit.onBodyPointerDown : undefined}
      onMouseDown={edit ? (event) => event.stopPropagation() : undefined}
      onDoubleClick={edit && item.kind === "note" ? (event) => { event.stopPropagation(); edit.onOpenNote(); } : undefined}
      onClick={interactive ? (event) => { event.stopPropagation(); onSelect?.(item.id); } : undefined}>
      {editing ? <NoteEditor text={edit!.draft!} edit={edit!} /> : <ItemContent item={item} live={live && isLiveFrameItem(item)} active={takesInput} theme={theme} />}
      {interactive && !selected && <button type="button" aria-label={`Select ${label}`} data-canvas-select="" className="absolute inset-0 border-0 bg-transparent p-0 focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]" style={{ cursor: "inherit" }} />}
      {edit && selected && !editing && <ResizeHandle edit={edit} />}
    </div>
  );
}
