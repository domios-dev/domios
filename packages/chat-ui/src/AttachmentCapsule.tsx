import { attachmentPathName as fileName } from "@codeterm/chat-core";
import { Fragment, useCallback, useContext, useId, useRef, useState, type ClipboardEvent, type CSSProperties, type HTMLAttributes, type ReactNode } from "react";
import { Button } from "./Button";
import { AnchoredPopover } from "./AnchoredPopover";
import { X, Maximize2, Mic, Diff, Film, File as FileIcon, FileText, Folder, Image as ImageIcon, MessagesSquare, Quote, ScrollText, type LucideIcon } from "lucide-react";
import { attachmentFileName, attachmentPreviewKind, DOMIOS_TEXT_MIME, type AttachmentKind, type AttachmentMarker, visibleAttachmentLabel, inlineAttachmentSrc, type ChatAttachment, type PlacedAttachmentSegment } from "@codeterm/chat-core";
import { capsuleSelectionText } from "./capsuleClipboard";
import { DsRenderContext } from "./blocks/DsRenderContext";
import { useDsSegments } from "./blocks/useDsSegments";
import { useLocalArtifactTabId } from "./LocalArtifactScope";
import { DsSegmentView } from "./blocks/DsSegmentView";
import { PRE_WRAP_CLASS } from "./CodeBlock";
import { VideoFullscreen } from "./VideoFullscreen";
import { ImageFullscreen, revealLocalFile, type FullscreenImage } from "./ImageFullscreen";
import { localArtifactOpeningAvailable } from "./localArtifactLoader";
import { useLocalImage, type LocalImage } from "./localImage";
import { useLocalTextPreview } from "./localText";

export const CAPSULE_ICONS: Record<AttachmentKind, LucideIcon> = {
  voice: Mic,
  image: ImageIcon,
  video: Film,
  text: ScrollText,
  file: FileIcon,
  folder: Folder,
  quote: Quote,
  message: MessagesSquare,
  diff: Diff,
  document: FileText,
};

const KIND_NAMES: Record<AttachmentKind, string> = {
  voice: "Voice", image: "Image", video: "Video", text: "Pasted text", file: "File", folder: "Folder", quote: "Quote", message: "Messages", diff: "Diff", document: "Document",
};
/** Kinds whose file is stored context, so the card names the capsule, not its temp file. */
const STORED_CONTEXT: ReadonlySet<AttachmentKind> = new Set(["quote", "message", "diff"]);

export const CAPSULE_DETAIL_STYLE: CSSProperties = { color: "var(--ct-muted-fg, #8b949e)" };

/** Plate and icon of a `.ct-capsule`: painted out of flow, so only the label's glyphs take width. */
export function CapsuleDecor({ kind }: { kind: AttachmentKind | null }) {
  const Icon = kind ? CAPSULE_ICONS[kind] : FileIcon;
  return (
    <>
      <span aria-hidden className="ct-capsule__plate" />
      <Icon aria-hidden className="ct-capsule__icon" />
    </>
  );
}

/** "text1 · 312 lines" → visible name "text", detail " · 312 lines"; the detail renders quieter. */
export function capsuleLabelParts(label: string): { name: string; detail: string } {
  const visible = visibleAttachmentLabel(label);
  const at = visible.indexOf(" · ");
  return at < 0 ? { name: visible, detail: "" } : { name: visible.slice(0, at), detail: visible.slice(at) };
}

/** The name assistive tech hears: a generated "image1" label gives way to the stored file name; a named label stays. */
export function attachmentAccessibleName(kind: AttachmentKind, path: string, label: string): string {
  const visible = visibleAttachmentLabel(label);
  return visible === label || STORED_CONTEXT.has(kind) || !path ? visible : fileName(path);
}

/** The card a hovered capsule shows: the image itself, the head of its text, or what the capsule points at. */
export function CapsulePreview({ kind, path, label, actions, onClose, closeLabel = "Close attachment preview", image: suppliedImage }: { kind: AttachmentKind; path: string; label: string; actions?: ReactNode; onClose?: () => void; closeLabel?: string; image?: LocalImage }) {
  const localImage = useLocalImage(kind === "image" && !suppliedImage ? path : undefined);
  const image = suppliedImage ?? localImage;
  const name = attachmentAccessibleName(kind, path, label);
  const snippet = useLocalTextPreview(attachmentPreviewKind({ kind, path }) === "document" ? path : undefined);
  const Icon = CAPSULE_ICONS[kind];
  const touchClose = onClose && typeof window !== "undefined" && window.matchMedia?.("(hover: none)").matches ? onClose : undefined;
  return (
    <div role="region" aria-label={`Preview ${name}`} data-copy-skip
      className="select-none flex w-max max-w-[min(24rem,80vw)] flex-col gap-1.5 overflow-hidden rounded-xl border p-1.5 text-xs shadow-lg backdrop-blur-md"
      style={{
        background: "color-mix(in srgb, var(--ct-surface, #161b22) 92%, transparent)",
        borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)",
        color: "var(--ct-fg)",
      }}
    >
      {(actions || touchClose) && <div role="group" aria-label="Preview actions" className="flex shrink-0 items-center justify-end gap-1">{actions}{touchClose && <Button variant="icon" aria-label={closeLabel} onClick={touchClose} style={{ minWidth: 44, minHeight: 44 }}><X size={18} aria-hidden /></Button>}</div>}
      {kind === "image" && (
        <div className="flex min-h-16 min-w-24 items-center justify-center overflow-hidden rounded-lg" style={{ background: "color-mix(in srgb, var(--ct-bg, #0d1117) 75%, transparent)" }}>
          {image.src
            ? <img src={image.src} alt={name} onError={image.markFailed} className="block max-h-40 max-w-72 object-contain" draggable={false} />
            : image.failed ? <span role="alert">Image unavailable <button type="button" aria-label={`Retry image ${name}`} onClick={image.retry}>Retry</button></span> : <ImageIcon size={18} aria-hidden className="opacity-40" />}
        </div>
      )}
      {kind !== "image" && <div className="flex min-w-0 items-center gap-1.5 px-1">
        <Icon size={12} aria-hidden className="shrink-0 opacity-70" />
        <span className="shrink-0 font-medium">{KIND_NAMES[kind]}</span>
        <span className="min-w-0 truncate font-mono text-[11px]" style={CAPSULE_DETAIL_STYLE} title={path}>{STORED_CONTEXT.has(kind) ? label : fileName(path)}</span>
      </div>}
      {snippet && (
        <pre
          data-testid="capsule-preview-text"
          className={`m-0 max-h-40 overflow-hidden whitespace-pre-wrap break-words rounded-lg px-2 py-1.5 text-[11px] leading-snug ${kind === "diff" ? "font-mono" : "font-[inherit]"}`}
          style={{ background: "color-mix(in srgb, var(--ct-bg, #0d1117) 75%, transparent)", color: "var(--ct-fg)" }}
        >
          {snippet}
        </pre>
      )}
    </div>
  );
}

interface AttachmentCapsuleProps {
  attachment: AttachmentMarker;
  image?: ChatAttachment;
  labelContent?: ReactNode;
  actions?: ReactNode;
  inert?: boolean;
  decoration?: ReactNode;
  spanProps?: HTMLAttributes<HTMLSpanElement> & Partial<Record<`data-${string}`, string | number | boolean | undefined>>;
}

function CapsuleContent({ attachment, labelContent, decoration, spanProps }: AttachmentCapsuleProps) {
  const label = attachment.label ?? fileName(attachment.path);
  const { name, detail } = capsuleLabelParts(label);
  const accessibleName = attachmentAccessibleName(attachment.kind, attachment.path, label);
  const tailAt = Math.max(0, name.length - 12);
  return <span title={attachment.path} aria-label={`${KIND_NAMES[attachment.kind]}: ${accessibleName}`} {...spanProps} className={labelContent ? "ct-capsule" : "ct-capsule ct-capsule--padded"}>
    {decoration ?? <CapsuleDecor kind={attachment.kind} />}
    {labelContent ?? <><span className="ct-capsule__name">{tailAt > 0 && <span>{name.slice(0, tailAt)}</span>}<span>{name.slice(tailAt)}</span></span>{detail && <span className="ct-capsule__detail">{detail}</span>}</>}
  </span>;
}

export function AttachmentCapsule(props: AttachmentCapsuleProps) {
  return props.inert ? <CapsuleContent {...props} /> : <InteractiveAttachmentCapsule {...props} />;
}

function InteractiveAttachmentCapsule({ attachment, image: nativeImage, labelContent, actions, decoration, spanProps }: AttachmentCapsuleProps) {
  const { kind, path } = attachment;
  const label = attachment.label ?? fileName(path);
  const accessibleName = attachmentAccessibleName(kind, path, label);
  const inline = nativeImage ? inlineAttachmentSrc(nativeImage) : null;
  const local = useLocalImage(kind === "image" && !inline ? path : undefined);
  const image = inline ? { ...local, src: inline, failed: false } : local;
  const [open, setOpen] = useState<FullscreenImage | null>(null);
  const [playing, setPlaying] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const anchorRef = useRef<HTMLSpanElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const previewRef = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(null), []);
  const opensImage = kind === "image" && Boolean(image.src);
  const restoringFocus = useRef(false);
  const skipPreviewFocus = useRef(false);
  const closePreview = useCallback(() => {
    restoringFocus.current = true;
    triggerRef.current?.focus({ preventScroll: true });
    restoringFocus.current = false;
    setPreviewing(false);
  }, []);
  return (
    <span ref={anchorRef} className="relative" onPointerEnter={(event) => { if (event.pointerType === "mouse") setPreviewing(true); }} onPointerLeave={(event) => { if (event.pointerType === "mouse" && (!(event.relatedTarget instanceof Node) || !previewRef.current?.contains(event.relatedTarget))) setPreviewing(false); }}>
      <button
        ref={triggerRef}
        type="button"
        aria-expanded={previewing && !open && !playing}
        data-testid="chat-inline-attachment"
        data-kind={kind}
        data-attachment-kind={kind}
        data-attachment-path={path}
        data-attachment-label={label}
        aria-label={kind === "image" ? `Open image ${accessibleName}` : kind === "video" ? `Play video ${accessibleName}` : `${localArtifactOpeningAvailable() ? "Reveal" : "Preview"} ${KIND_NAMES[kind].toLowerCase()} ${accessibleName}`}
        title={path}
        onFocus={() => { if (skipPreviewFocus.current) { skipPreviewFocus.current = false; return; } if (!restoringFocus.current) setPreviewing(true); }}
        onBlur={(event) => { if (!previewRef.current?.contains(event.relatedTarget)) setPreviewing(false); }}
        onClick={(event) => {
          if (event.detail > 0 && window.matchMedia?.("(hover: none)").matches) { setPreviewing(true); return; }
          event.currentTarget.focus();
          setPreviewing(false);
          if (opensImage || kind === "video") skipPreviewFocus.current = true;
          if (opensImage && image.src) setOpen({ name: nativeImage ? attachmentFileName(nativeImage) : fileName(path), src: image.src, path });
          else if (kind === "video") setPlaying(true);
          else if (kind === "image") setPreviewing(true);
          else if (localArtifactOpeningAvailable()) revealLocalFile(path);
          else setPreviewing(true);
        }}
        className="ct-capsule-button pointer-events-auto"
      >
        <CapsuleContent attachment={attachment} labelContent={labelContent} decoration={decoration} spanProps={spanProps} />
      </button>
      <AnchoredPopover open={previewing && !open && !playing} anchor={triggerRef} floatingRef={previewRef} placement="top-start" gap={0} onClose={closePreview} style={{ overflow: "auto" }} onPointerEnter={() => setPreviewing(true)} onPointerLeave={(event) => { if (event.pointerType === "mouse") setPreviewing(false); }}>
          <CapsulePreview kind={kind} path={path} label={label} image={image} onClose={closePreview} closeLabel={`Close preview ${accessibleName}`} actions={<>{actions}
            {(opensImage || kind === "video") && <Button variant="icon" aria-label={`Expand ${kind} ${accessibleName}`} title="Expand" onClick={() => { triggerRef.current?.focus(); skipPreviewFocus.current = true; setPreviewing(false); if (kind === "video") setPlaying(true); else if (image.src) setOpen({ name: fileName(path), src: image.src, path }); }} style={{ minWidth: 44, minHeight: 44 }}><Maximize2 size={16} aria-hidden /></Button>}
          </>} />
      </AnchoredPopover>
      {open && <ImageFullscreen image={open} anchorRef={anchorRef} onClose={close} />}
      {playing && <VideoFullscreen label={label} path={path} anchorRef={anchorRef} onClose={() => setPlaying(false)} />}
    </span>
  );
}

/** User text with its capsules rendered where they were typed; a copy keeps capsules as tags for a composer. */
export function InlineAttachmentText({ segments, renderText }: { segments: PlacedAttachmentSegment[]; renderText?: (text: string) => ReactNode }) {
  const scope = useContext(DsRenderContext);
  const instance = useId();
  const tab = useLocalArtifactTabId();
  let source = "";
  const attachments: Array<{ offset: number; index: number; segment: Extract<PlacedAttachmentSegment, { attachment: AttachmentMarker }> }> = [];
  for (const [index, segment] of segments.entries()) {
    if ("text" in segment) source += segment.text;
    else { attachments.push({ offset: source.length, index, segment }); source += "\0"; }
  }
  const parsed = useDsSegments(source, { id: `${scope.authority}:${tab ?? ""}:${instance}`, trusted: scope.trusted }, scope.streaming, scope.cancelled);
  const children = parsed.flatMap((part): ReactNode[] => {
    const capsules = attachments.filter(({ offset }) => offset >= part.start && offset < part.end);
    if (part.type !== "markdown" && capsules.length === 0) return [<DsSegmentView key={part.id} segment={part} />];
    const result: ReactNode[] = [];
    let cursor = part.start;
    const text = (start: number, end: number) => {
      if (end <= start) return;
      const value = source.slice(start, end);
      result.push(<Fragment key={`text:${start}`}>{part.type === "markdown" && renderText ? renderText(value) : value}</Fragment>);
    };
    for (const capsule of capsules) {
      text(cursor, capsule.offset);
      result.push(<AttachmentCapsule key={`${capsule.index}:${capsule.segment.attachment.path}`} attachment={capsule.segment.attachment} image={capsule.segment.image} />);
      cursor = capsule.offset + 1;
    }
    text(cursor, part.end);
    return result;
  });
  const copy = (event: ClipboardEvent<HTMLDivElement>) => {
    const selection = window.getSelection();
    if (!selection || selection.rangeCount !== 1 || selection.isCollapsed) return;
    const range = selection.getRangeAt(0);
    if (!event.currentTarget.contains(range.commonAncestorContainer)) return;
    const { plain, portable } = capsuleSelectionText(range.cloneContents());
    if (plain === portable) return;
    event.preventDefault();
    event.clipboardData.setData("text/plain", plain);
    event.clipboardData.setData(DOMIOS_TEXT_MIME, portable);
  };
  return (
    <div data-testid="chat-inline-attachment-text" className={`${PRE_WRAP_CLASS} m-0 select-text font-[inherit]`} onCopy={copy}>
      {children}
    </div>
  );
}
