import { useCallback, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { File as FileIcon, Folder, Image as ImageIcon, ScrollText, type LucideIcon } from "lucide-react";
import type { AttachmentKind, AttachmentMarker, AttachmentSegment } from "@codeterm/chat-core";
import { PRE_WRAP_CLASS } from "./CodeBlock";
import { ImageFullscreen, REVEAL_UNAVAILABLE, revealLocalFile, type FullscreenImage } from "./ImageFullscreen";
import { localArtifactOpeningAvailable } from "./localArtifactLoader";
import { useLocalImage } from "./localImage";

export const CAPSULE_ICONS: Record<AttachmentKind, LucideIcon> = {
  image: ImageIcon,
  text: ScrollText,
  file: FileIcon,
  folder: Folder,
};

const KIND_NAMES: Record<AttachmentKind, string> = { image: "Image", text: "Pasted text", file: "File", folder: "Folder" };

/** Fill and ring only: a capsule painted over composer text must never change its width. */
export const CAPSULE_TONE: CSSProperties = {
  background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 15%, transparent)",
  boxShadow: "inset 0 0 0 1px color-mix(in srgb, var(--ct-accent, #58a6ff) 42%, transparent)",
  color: "color-mix(in srgb, var(--ct-accent, #58a6ff) 55%, var(--ct-fg, #e6edf3))",
  borderRadius: "0.45em",
};

export const CAPSULE_DETAIL_STYLE: CSSProperties = { color: "var(--ct-muted-fg, #8b949e)" };

/** "text1 · 312 lines" → name "text1", detail " · 312 lines"; the detail renders quieter. */
export function capsuleLabelParts(label: string): { name: string; detail: string } {
  const at = label.indexOf(" · ");
  return at < 0 ? { name: label, detail: "" } : { name: label.slice(0, at), detail: label.slice(at) };
}

function fileName(path: string): string {
  return path.replace(/[\\/]+$/, "").split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** The card a hovered capsule shows: the image itself, or what the capsule points at. */
export function CapsulePreview({ kind, path, label, actions }: { kind: AttachmentKind; path: string; label: string; actions?: ReactNode }) {
  const image = useLocalImage(kind === "image" ? path : undefined);
  const Icon = CAPSULE_ICONS[kind];
  return (
    <div
      className="flex w-max max-w-[min(20rem,80vw)] flex-col gap-1.5 overflow-hidden rounded-xl border p-1.5 text-xs shadow-lg backdrop-blur-md"
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
        <span className="min-w-0 truncate font-mono text-[11px]" style={CAPSULE_DETAIL_STYLE} title={path}>{fileName(path)}</span>
        {actions}
      </div>
    </div>
  );
}

/** A capsule the user typed inline: images open fullscreen, everything else reveals on disk. */
export function AttachmentCapsule({ attachment }: { attachment: AttachmentMarker }) {
  const { kind, path } = attachment;
  const label = attachment.label ?? fileName(path);
  const { name, detail } = capsuleLabelParts(label);
  const Icon = CAPSULE_ICONS[kind];
  const image = useLocalImage(kind === "image" ? path : undefined);
  const [open, setOpen] = useState<FullscreenImage | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const anchorRef = useRef<HTMLSpanElement>(null);
  const close = useCallback(() => setOpen(null), []);
  const canReveal = localArtifactOpeningAvailable();
  const opensImage = kind === "image" && Boolean(image.src);
  const disabled = !opensImage && (kind === "image" || !canReveal);
  return (
    <span ref={anchorRef} className="relative inline-block align-baseline" onMouseEnter={() => setPreviewing(true)} onMouseLeave={() => setPreviewing(false)}>
      <button
        type="button"
        data-testid="chat-inline-attachment"
        data-kind={kind}
        aria-label={kind === "image" ? `Open image ${label}` : `Reveal ${KIND_NAMES[kind].toLowerCase()} ${label}`}
        title={disabled && kind !== "image" ? `${path} (${REVEAL_UNAVAILABLE})` : path}
        disabled={disabled}
        onFocus={() => setPreviewing(true)}
        onBlur={() => setPreviewing(false)}
        onClick={() => {
          if (opensImage && image.src) setOpen({ name: fileName(path), src: image.src, path });
          else revealLocalFile(path);
        }}
        className="ct-liquid-interactive mx-px inline-flex max-w-[18rem] items-center gap-1 px-1.5 align-baseline font-[inherit] leading-[1.35] disabled:cursor-default focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--ct-accent)]"
        style={CAPSULE_TONE}
      >
        <Icon size={12} aria-hidden className="shrink-0 opacity-75" />
        <span className="min-w-0 truncate">{name}<span style={CAPSULE_DETAIL_STYLE}>{detail}</span></span>
      </button>
      {previewing && !open && (
        <span role="presentation" className="pointer-events-none absolute bottom-full left-0 z-20 block pb-1.5">
          <CapsulePreview kind={kind} path={path} label={label} />
        </span>
      )}
      {open && <ImageFullscreen image={open} anchorRef={anchorRef} onClose={close} />}
    </span>
  );
}

/** User text with its capsules rendered where they were typed. */
export function InlineAttachmentText({ segments }: { segments: AttachmentSegment[] }) {
  return (
    <pre data-testid="chat-inline-attachment-text" className={`${PRE_WRAP_CLASS} m-0 select-text font-[inherit]`}>
      {segments.map((segment, index) => ("text" in segment
        ? segment.text
        : <AttachmentCapsule key={`${index}:${segment.attachment.path}`} attachment={segment.attachment} />))}
    </pre>
  );
}
