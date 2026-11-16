import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import type { DsCanvasItem } from "@codeterm/chat-core";
import { buildHtmlPreviewDocument, sanitizeLocalHtml, SandboxedHtmlFrame, type ThemeVars } from "../HtmlPreviewBlock";
import { loadLocalArtifact } from "../localArtifactLoader";
import { loadHtmlFile } from "../htmlFileLoader";
import { useLocalArtifactTabId } from "../LocalArtifactScope";
import { RichMarkdown } from "../MarkdownBody";
import { canvasItemLabel, isLiveFrameItem, type PlacedCanvasItem } from "./canvasBlockModel";

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

async function readItemFile(item: Extract<DsCanvasItem, { src: string }>, tabId: string | undefined): Promise<FileContent> {
  if (item.kind === "image") {
    const file = await loadLocalArtifact(item.src, "image", tabId);
    if (!file.data.startsWith("data:image/")) throw new Error("The file is not an image.");
    return { kind: "image", src: file.data };
  }
  if (item.kind === "page" && !isMarkdownPath(item.src)) return { kind: "html", html: sanitizeLocalHtml(await loadHtmlFile(item.src, tabId)).html };
  const file = await loadLocalArtifact(item.src, "document", tabId);
  return { kind: "text", text: file.data, markdown: file.media_type === "text/markdown" };
}

function useItemFile(item: DsCanvasItem): FileState | null {
  const tabId = useLocalArtifactTabId();
  const src = "src" in item ? item.src : null;
  const [state, setState] = useState<{ key: string; value: FileState } | null>(null);
  const key = `${item.kind}:${src}:${tabId ?? ""}`;
  useEffect(() => {
    if (!("src" in item)) return;
    let current = true;
    const read = `${item.kind}:${item.src}:${tabId ?? ""}`;
    readItemFile(item, tabId).then(
      (content) => { if (current) setState({ key: read, value: { status: "ready", content } }); },
      (error: unknown) => { if (current) setState({ key: read, value: { status: "error", reason: error instanceof Error ? error.message : String(error) } }); },
    );
    return () => { current = false; };
  }, [item, tabId]);
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
  const inlineDoc = useMemo(() => item.kind === "html" ? buildHtmlPreviewDocument(item.html, theme) : null, [item, theme]);
  const pageDoc = useMemo(() => file?.status === "ready" && file.content.kind === "html" ? buildHtmlPreviewDocument(file.content.html, theme) : null, [file, theme]);
  if (item.kind === "note") return <TextBox scrolls={active} fill><Markdown text={item.markdown} /></TextBox>;
  if (file?.status === "error") return <CanvasItemFailure text={FAILURE_TEXT[item.kind]} reason={file.reason} />;
  if (file?.status === "loading") return <div role="status" aria-label={`Loading ${label}`} className="h-full w-full" />;
  const doc = inlineDoc ?? pageDoc;
  if (doc !== null) {
    if (!live) return <FramePlaceholder label={label} />;
    const style: CSSProperties = { display: "block", width: "100%", height: "100%", border: "1px solid var(--ct-border)", boxSizing: "border-box", background: "var(--ct-bg)", pointerEvents: active ? "auto" : "none" };
    return <SandboxedHtmlFrame doc={doc} title={label} testId="chat-canvas-frame" style={style} />;
  }
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
export function CanvasItemView({ placed, live, selected = false, interactive, theme, onSelect, itemRef }: {
  itemRef?: (element: HTMLDivElement | null) => void;
  placed: PlacedCanvasItem;
  live: boolean;
  selected?: boolean;
  interactive: boolean;
  theme: ThemeVars;
  onSelect?: (id: string) => void;
}) {
  const { item } = placed;
  const label = canvasItemLabel(item);
  const takesInput = interactive && selected;
  const outline = selected ? { outline: "2px solid var(--ct-accent)" } : {};
  return (
    <div ref={itemRef} data-canvas-item={item.id} data-zoom-target={interactive ? placed.index : undefined} data-selected={selected || undefined}
      role={interactive ? "group" : undefined} aria-label={interactive ? label : undefined}
      className={`${ITEM_RING} ${interactive && !selected ? "hover:outline hover:outline-1 hover:outline-[var(--ct-border)]" : ""}`}
      style={{ position: "absolute", left: placed.x, top: placed.y, width: placed.w, height: placed.h, ...outline }}
      onClick={interactive ? (event) => { event.stopPropagation(); onSelect?.(item.id); } : undefined}>
      <ItemContent item={item} live={live && isLiveFrameItem(item)} active={takesInput} theme={theme} />
      {interactive && !selected && <button type="button" aria-label={`Select ${label}`} data-canvas-select="" className="absolute inset-0 border-0 bg-transparent p-0 focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]" style={{ cursor: "inherit" }} />}
    </div>
  );
}
