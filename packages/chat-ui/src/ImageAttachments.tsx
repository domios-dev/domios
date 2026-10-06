import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Download, Image as ImageIcon, X } from "lucide-react";
import { attachmentFileName, inlineAttachmentSrc, type ChatAttachment } from "@codeterm/chat-core";
import { loadLocalArtifact } from "./localArtifactLoader";

interface OpenImage {
  name: string;
  src: string;
}

function useAttachmentSrc(attachment: ChatAttachment): string | null {
  const inline = inlineAttachmentSrc(attachment);
  const path = inline ? undefined : attachment.path;
  const [loaded, setLoaded] = useState<{ path: string; src: string | null } | null>(null);
  useEffect(() => {
    if (!path) return;
    let live = true;
    loadLocalArtifact(path, "image").then(
      (file) => { if (live) setLoaded({ path, src: file.data }); },
      () => { if (live) setLoaded({ path, src: null }); },
    );
    return () => { live = false; };
  }, [path]);
  return inline ?? (loaded && loaded.path === path ? loaded.src : null);
}

function AttachmentThumbnail({ attachment, onOpen }: { attachment: ChatAttachment; onOpen: (image: OpenImage) => void }) {
  const src = useAttachmentSrc(attachment);
  const name = attachmentFileName(attachment);
  return (
    <button
      type="button"
      data-testid="chat-image-attachment-button"
      aria-label={`Open image ${name}`}
      title={src ? name : `${name} (not available)`}
      disabled={!src}
      onClick={() => { if (src) onOpen({ name, src }); }}
      className="inline-flex items-center gap-1 overflow-hidden rounded-md text-[11px] transition-colors hover:brightness-125 disabled:cursor-default"
      style={{
        background: "color-mix(in srgb, var(--ct-accent) 15%, transparent)",
        color: "var(--ct-muted-fg)",
      }}
    >
      {src ? (
        <img src={src} alt="" className="block h-24 max-w-[200px] object-cover" draggable={false} />
      ) : (
        <span className="inline-flex items-center gap-1 px-1.5 py-0.5">
          <ImageIcon size={12} strokeWidth={2} aria-hidden />
          {attachment.mimeType.replace("image/", "").toUpperCase()}
        </span>
      )}
    </button>
  );
}

function ImageViewer({ image, onClose }: { image: OpenImage; onClose: () => void }) {
  const closeRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    closeRef.current?.focus();
    // Capture phase: Escape must close the viewer, not reach the chat's own Escape handling.
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      opener?.focus?.();
    };
  }, [onClose]);

  return createPortal(
    <div
      role="dialog"
      aria-modal="true"
      aria-label={`Image ${image.name}`}
      data-testid="chat-image-preview"
      className="fixed inset-0 z-[1000] flex flex-col items-center justify-center gap-3 p-6"
      style={{ background: "rgba(0, 0, 0, 0.82)" }}
      onClick={(event) => { if (event.target === event.currentTarget) onClose(); }}
    >
      <img src={image.src} alt={image.name} className="max-h-[82vh] max-w-[92vw] rounded-lg object-contain" />
      <div className="flex items-center gap-2 text-xs" style={{ color: "#e6edf3" }}>
        <span className="max-w-[50vw] truncate opacity-80">{image.name}</span>
        <a
          href={image.src}
          download={image.name}
          className="inline-flex items-center gap-1 rounded px-2 py-1 hover:brightness-125"
          style={{ background: "rgba(255, 255, 255, 0.12)" }}
        >
          <Download size={12} strokeWidth={2} aria-hidden />
          Save
        </a>
        <button
          ref={closeRef}
          type="button"
          onClick={onClose}
          aria-label="Close image"
          className="inline-flex items-center gap-1 rounded px-2 py-1 hover:brightness-125"
          style={{ background: "rgba(255, 255, 255, 0.12)" }}
        >
          <X size={12} strokeWidth={2} aria-hidden />
          Close
        </button>
      </div>
    </div>,
    document.body,
  );
}

export function ImageAttachments({ attachments, separated }: { attachments: ChatAttachment[]; separated: boolean }) {
  const [open, setOpen] = useState<OpenImage | null>(null);
  const close = useCallback(() => setOpen(null), []);
  return (
    <>
      <div
        className={`flex flex-wrap gap-1.5 ${separated ? "mt-2 pt-2" : ""}`}
        style={separated ? { borderTop: "1px solid color-mix(in srgb, var(--ct-accent) 15%, transparent)" } : undefined}
      >
        {attachments.map((attachment) => (
          <AttachmentThumbnail key={attachment.id} attachment={attachment} onOpen={setOpen} />
        ))}
      </div>
      {open && <ImageViewer image={open} onClose={close} />}
    </>
  );
}
