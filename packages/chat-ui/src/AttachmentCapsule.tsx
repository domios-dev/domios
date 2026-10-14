import { useCallback, useRef, useState, type ClipboardEvent, type CSSProperties, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Mic, Diff, File as FileIcon, FileText, Folder, Image as ImageIcon, MessagesSquare, Quote, ScrollText, type LucideIcon } from "lucide-react";
import { attachmentFileName, attachmentPreviewKind, inlineArtifactParts, DOMIOS_TEXT_MIME, type AttachmentKind, type AttachmentMarker, inlineAttachmentSrc, type ChatAttachment, type PlacedAttachmentSegment } from "@codeterm/chat-core";
import { capsuleSelectionText } from "./capsuleClipboard";
import { LocalArtifactBlock } from "./LocalArtifactBlock";
import { RichMarkdown } from "./MarkdownBody";
import { PRE_WRAP_CLASS } from "./CodeBlock";
import { ImageFullscreen, REVEAL_UNAVAILABLE, revealLocalFile, type FullscreenImage } from "./ImageFullscreen";
import { localArtifactOpeningAvailable } from "./localArtifactLoader";
import { useLocalImage, type LocalImage } from "./localImage";
import { useLocalTextPreview } from "./localText";

export const CAPSULE_ICONS: Record<AttachmentKind, LucideIcon> = {
  voice: Mic,
  image: ImageIcon,
  text: ScrollText,
  file: FileIcon,
  folder: Folder,
  quote: Quote,
  message: MessagesSquare,
  diff: Diff,
  document: FileText,
};

const KIND_NAMES: Record<AttachmentKind, string> = {
  voice: "Voice", image: "Image", text: "Pasted text", file: "File", folder: "Folder", quote: "Quote", message: "Messages", diff: "Diff", document: "Document",
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

/** "text1 · 312 lines" → name "text1", detail " · 312 lines"; the detail renders quieter. */
export function capsuleLabelParts(label: string): { name: string; detail: string } {
  const at = label.indexOf(" · ");
  return at < 0 ? { name: label, detail: "" } : { name: label.slice(0, at), detail: label.slice(at) };
}

function fileName(path: string): string {
  return path.replace(/[\\/]+$/, "").split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** The card a hovered capsule shows: the image itself, the head of its text, or what the capsule points at. */
export function CapsulePreview({ kind, path, label, actions, image: suppliedImage }: { kind: AttachmentKind; path: string; label: string; actions?: ReactNode; image?: LocalImage }) {
  const localImage = useLocalImage(kind === "image" && !suppliedImage ? path : undefined);
  const image = suppliedImage ?? localImage;
  const snippet = useLocalTextPreview(attachmentPreviewKind({ kind, path }) === "document" ? path : undefined);
  const Icon = CAPSULE_ICONS[kind];
  return (
    <div
      className="flex w-max max-w-[min(24rem,80vw)] flex-col gap-1.5 overflow-hidden rounded-xl border p-1.5 text-xs shadow-lg backdrop-blur-md"
      style={{
        background: "color-mix(in srgb, var(--ct-surface, #161b22) 92%, transparent)",
        borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)",
        color: "var(--ct-fg)",
      }}
    >
      {kind === "image" && (
        <div className="flex min-h-16 min-w-24 items-center justify-center overflow-hidden rounded-lg" style={{ background: "color-mix(in srgb, var(--ct-bg, #0d1117) 75%, transparent)" }}>
          {image.src
            ? <img src={image.src} alt={label} onError={image.markFailed} className="block max-h-40 max-w-72 object-contain" draggable={false} />
            : image.failed ? <span role="alert">Image unavailable <button type="button" aria-label={`Retry image ${label}`} onClick={image.retry}>Retry</button></span> : <ImageIcon size={18} aria-hidden className="opacity-40" />}
        </div>
      )}
      <div className="flex min-w-0 items-center gap-1.5 px-1">
        <Icon size={12} aria-hidden className="shrink-0 opacity-70" />
        <span className="shrink-0 font-medium">{KIND_NAMES[kind]}</span>
        <span className="min-w-0 truncate font-mono text-[11px]" style={CAPSULE_DETAIL_STYLE} title={path}>{STORED_CONTEXT.has(kind) ? label : fileName(path)}</span>
        {actions}
      </div>
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

/** A capsule the user typed inline: images open fullscreen, everything else reveals on disk. */
export function AttachmentCapsule({ attachment, image: nativeImage, labelContent, actions }: { attachment: AttachmentMarker; image?: ChatAttachment; labelContent?: ReactNode; actions?: ReactNode }) {
  const { kind, path } = attachment;
  const label = attachment.label ?? fileName(path);
  const { name, detail } = capsuleLabelParts(label);
  const inline = nativeImage ? inlineAttachmentSrc(nativeImage) : null;
  const local = useLocalImage(kind === "image" && !inline ? path : undefined);
  const image = inline ? { ...local, src: inline, failed: false } : local;
  const [open, setOpen] = useState<FullscreenImage | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const anchorRef = useRef<HTMLSpanElement>(null);
  const previewRef = useRef<HTMLSpanElement>(null);
  const close = useCallback(() => setOpen(null), []);
  const canReveal = localArtifactOpeningAvailable();
  const opensImage = kind === "image" && Boolean(image.src);
  const disabled = kind !== "image" && !canReveal;
  const bounds = previewing ? anchorRef.current?.getBoundingClientRect() : undefined;
  return (
    <span ref={anchorRef} className="relative" onMouseEnter={() => setPreviewing(true)} onMouseLeave={(event) => { if (!(event.relatedTarget instanceof Node) || !previewRef.current?.contains(event.relatedTarget)) setPreviewing(false); }}>
      <button
        type="button"
        data-testid="chat-inline-attachment"
        data-kind={kind}
        data-attachment-kind={kind}
        data-attachment-path={path}
        data-attachment-label={label}
        aria-label={kind === "image" ? `Open image ${label}` : `Reveal ${KIND_NAMES[kind].toLowerCase()} ${label}`}
        title={disabled ? `${path} (${REVEAL_UNAVAILABLE})` : path}
        disabled={disabled}
        onFocus={() => setPreviewing(true)}
        onBlur={(event) => { if (!previewRef.current?.contains(event.relatedTarget)) setPreviewing(false); }}
        onClick={() => {
          if (opensImage && image.src) setOpen({ name: nativeImage ? attachmentFileName(nativeImage) : fileName(path), src: image.src, path });
          else if (kind === "image") setPreviewing(true);
          else revealLocalFile(path);
        }}
        className="ct-capsule-button pointer-events-auto"
      >
        <span className={labelContent ? "ct-capsule" : "ct-capsule ct-capsule--padded"}>
          <CapsuleDecor kind={kind} />
          {labelContent ?? <>{name}{detail && <span className="ct-capsule__detail">{detail}</span>}</>}
        </span>
      </button>
      {previewing && !open && createPortal(
        <span ref={previewRef} role="region" aria-label={`Preview ${label}`} data-copy-skip className="fixed z-50 block select-none pb-1.5" style={{ left: Math.max(0, Math.min(bounds?.left ?? 0, window.innerWidth - 300)), ...(bounds && bounds.top < 200 ? { top: bounds.bottom } : { bottom: window.innerHeight - (bounds?.top ?? 0) }) }} onMouseEnter={() => setPreviewing(true)} onMouseLeave={(event) => { if (!(event.relatedTarget instanceof Node) || !previewRef.current?.contains(event.relatedTarget)) setPreviewing(false); }}>
          <CapsulePreview kind={kind} path={path} label={label} image={image} actions={actions} />
        </span>, document.body
      )}
      {open && <ImageFullscreen image={open} anchorRef={anchorRef} onClose={close} />}
    </span>
  );
}

/** User text with its capsules rendered where they were typed; a copy keeps capsules as tags for a composer. */
export function InlineAttachmentText({ segments }: { segments: PlacedAttachmentSegment[] }) {
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
      {inlineArtifactParts(segments).map((segment, index) => ("directive" in segment
        ? <LocalArtifactBlock key={index} directive={segment.directive} renderMarkdown={(text) => <RichMarkdown text={text} localArtifacts={false} />} />
        : "text" in segment
        ? segment.text
        : <AttachmentCapsule key={`${index}:${segment.attachment.path}`} attachment={segment.attachment} image={segments.flatMap((part) => "attachment" in part && part.attachment === segment.attachment && part.image ? [part.image] : [])[0]} />))}
    </div>
  );
}
