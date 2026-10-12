import { useCallback, useRef, useState, type ClipboardEvent, type CSSProperties, type ReactNode } from "react";
import { Diff, File as FileIcon, FileText, Folder, Image as ImageIcon, MessagesSquare, Quote, ScrollText, type LucideIcon } from "lucide-react";
import { attachmentPreviewKind, inlineArtifactParts, DOMIOS_TEXT_MIME, type AttachmentKind, type AttachmentMarker, type AttachmentSegment } from "@codeterm/chat-core";
import { capsuleSelectionText } from "./capsuleClipboard";
import { LocalArtifactBlock } from "./LocalArtifactBlock";
import { RichMarkdown } from "./MarkdownBody";
import { PRE_WRAP_CLASS } from "./CodeBlock";
import { ImageFullscreen, REVEAL_UNAVAILABLE, revealLocalFile, type FullscreenImage } from "./ImageFullscreen";
import { localArtifactOpeningAvailable } from "./localArtifactLoader";
import { useLocalImage } from "./localImage";
import { useLocalTextPreview } from "./localText";

export const CAPSULE_ICONS: Record<AttachmentKind, LucideIcon> = {
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
  image: "Image", text: "Pasted text", file: "File", folder: "Folder", quote: "Quote", message: "Messages", diff: "Diff", document: "Document",
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
export function CapsulePreview({ kind, path, label, actions }: { kind: AttachmentKind; path: string; label: string; actions?: ReactNode }) {
  const image = useLocalImage(kind === "image" ? path : undefined);
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
            : <ImageIcon size={18} aria-hidden className={image.failed ? "opacity-70" : "opacity-40"} />}
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
export function AttachmentCapsule({ attachment }: { attachment: AttachmentMarker }) {
  const { kind, path } = attachment;
  const label = attachment.label ?? fileName(path);
  const { name, detail } = capsuleLabelParts(label);
  const image = useLocalImage(kind === "image" ? path : undefined);
  const [open, setOpen] = useState<FullscreenImage | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const anchorRef = useRef<HTMLSpanElement>(null);
  const close = useCallback(() => setOpen(null), []);
  const canReveal = localArtifactOpeningAvailable();
  const opensImage = kind === "image" && Boolean(image.src);
  const disabled = !opensImage && (kind === "image" || !canReveal);
  return (
    <span ref={anchorRef} className="relative" onMouseEnter={() => setPreviewing(true)} onMouseLeave={() => setPreviewing(false)}>
      <button
        type="button"
        data-testid="chat-inline-attachment"
        data-kind={kind}
        data-attachment-kind={kind}
        data-attachment-path={path}
        data-attachment-label={label}
        aria-label={kind === "image" ? `Open image ${label}` : `Reveal ${KIND_NAMES[kind].toLowerCase()} ${label}`}
        title={disabled && kind !== "image" ? `${path} (${REVEAL_UNAVAILABLE})` : path}
        disabled={disabled}
        onFocus={() => setPreviewing(true)}
        onBlur={() => setPreviewing(false)}
        onClick={() => {
          if (opensImage && image.src) setOpen({ name: fileName(path), src: image.src, path });
          else revealLocalFile(path);
        }}
        className="ct-capsule-button"
      >
        <span className="ct-capsule ct-capsule--padded">
          <CapsuleDecor kind={kind} />
          {kind === "image" && image.src && <img src={image.src} alt="" onError={image.markFailed} draggable={false} className="mr-1 inline-block h-12 max-w-24 rounded-md object-cover align-middle" />}{name}{detail && <span className="ct-capsule__detail">{detail}</span>}
        </span>
      </button>
      {previewing && !open && (
        <span role="presentation" data-copy-skip className="pointer-events-none absolute bottom-full left-0 z-20 block select-none pb-1.5">
          <CapsulePreview kind={kind} path={path} label={label} />
        </span>
      )}
      {open && <ImageFullscreen image={open} anchorRef={anchorRef} onClose={close} />}
    </span>
  );
}

/** User text with its capsules rendered where they were typed; a copy keeps capsules as tags for a composer. */
export function InlineAttachmentText({ segments }: { segments: AttachmentSegment[] }) {
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
        : <AttachmentCapsule key={`${index}:${segment.attachment.path}`} attachment={segment.attachment} />))}
    </div>
  );
}
