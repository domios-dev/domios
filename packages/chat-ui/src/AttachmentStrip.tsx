import { useCallback, useRef, useState } from "react";
import { FileText, Image as ImageIcon } from "lucide-react";
import { attachmentFileName, inlineAttachmentSrc, type AttachmentMarker, type ChatAttachment } from "@codeterm/chat-core";
import { ImageFullscreen, REVEAL_UNAVAILABLE, revealLocalFile, type FullscreenImage } from "./ImageFullscreen";
import { localArtifactOpeningAvailable } from "./localArtifactLoader";
import { useLocalImage } from "./localImage";

const TILE_BORDER = "color-mix(in srgb, var(--ct-accent, #58a6ff) 38%, transparent)";

function ImageTile({ attachment, large, onOpen }: { attachment: ChatAttachment; large: boolean; onOpen: (image: FullscreenImage) => void }) {
  const inline = inlineAttachmentSrc(attachment);
  const local = useLocalImage(inline ? undefined : attachment.path);
  const src = inline ?? local.src;
  const name = attachmentFileName(attachment);
  const height = large ? "h-36" : "h-16";
  return (
    <button
      type="button"
      data-testid="chat-image-attachment-button"
      aria-label={`Open image ${name}`}
      title={src ? name : `${name} (not available)`}
      disabled={!src}
      onClick={() => { if (src) onOpen({ name, src, path: attachment.path }); }}
      className={`ct-liquid-interactive relative inline-flex ${height} shrink-0 items-center justify-center overflow-hidden rounded-xl border disabled:cursor-default focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]`}
      style={{
        borderColor: TILE_BORDER,
        background: "color-mix(in srgb, var(--ct-bg, #0d1117) 70%, transparent)",
        boxShadow: "0 1px 0 color-mix(in srgb, var(--ct-fg, #e6edf3) 8%, transparent) inset",
      }}
    >
      {src ? (
        <img
          src={src}
          alt=""
          onError={local.markFailed}
          className={`block h-full w-auto ${large ? "min-w-24 max-w-[18rem]" : "min-w-16 max-w-[9rem]"} object-cover`}
          draggable={false}
        />
      ) : (
        <span className="inline-flex h-full w-16 flex-col items-center justify-center gap-1 text-[10px]" style={{ color: "var(--ct-muted-fg)" }}>
          <ImageIcon size={16} strokeWidth={1.75} aria-hidden className={local.failed || inline ? "" : "opacity-50"} />
          {attachment.mimeType.replace("image/", "").split("+")[0].toUpperCase()}
        </span>
      )}
    </button>
  );
}

function FileChip({ file }: { file: AttachmentMarker }) {
  const name = file.path.split(/[\\/]/).filter(Boolean).pop() ?? file.path;
  const canReveal = localArtifactOpeningAvailable();
  return (
    <button
      type="button"
      data-testid="chat-file-attachment"
      aria-label={`Reveal file ${name}`}
      title={canReveal ? file.path : `${file.path} (${REVEAL_UNAVAILABLE})`}
      disabled={!canReveal}
      onClick={() => revealLocalFile(file.path)}
      className="ct-liquid-interactive disabled:cursor-default inline-flex h-7 min-w-0 max-w-full items-center gap-1.5 rounded-full border px-2.5 font-mono text-xs focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]"
      style={{
        background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, transparent)",
        borderColor: TILE_BORDER,
        color: "var(--ct-fg)",
      }}
    >
      <FileText size={12} aria-hidden className="shrink-0 opacity-70" />
      <span className="min-w-0 max-w-[14rem] truncate">{name}</span>
    </button>
  );
}

/** Attachments of a user message: image thumbnails open fullscreen, file chips reveal the file. */
export function AttachmentStrip({ images, files, captioned }: { images: ChatAttachment[]; files: AttachmentMarker[]; captioned: boolean }) {
  const [open, setOpen] = useState<FullscreenImage | null>(null);
  const close = useCallback(() => setOpen(null), []);
  const stripRef = useRef<HTMLDivElement>(null);
  const large = images.length === 1 && files.length === 0;
  return (
    <>
      <div
        ref={stripRef}
        role="group"
        aria-label="Attachments"
        data-testid="chat-attachment-strip"
        className={`flex flex-wrap items-end gap-1.5 ${captioned ? "mb-2" : ""}`}
      >
        {images.map((attachment) => (
          <ImageTile key={attachment.id} attachment={attachment} large={large} onOpen={setOpen} />
        ))}
        {files.map((file) => <FileChip key={file.path} file={file} />)}
      </div>
      {open && <ImageFullscreen image={open} anchorRef={stripRef} onClose={close} />}
    </>
  );
}
