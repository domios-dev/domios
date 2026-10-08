import { useRef, type RefObject } from "react";
import { createPortal } from "react-dom";
import { Download, FolderOpen, Image as ImageIcon, X } from "lucide-react";
import { FullscreenOverlay, useFullscreenHostBounds, useFullscreenKeyboard } from "./FullscreenOverlay";
import { localArtifactOpeningAvailable, openLocalArtifact } from "./localArtifactLoader";

export interface FullscreenImage {
  name: string;
  src: string;
  path?: string;
}

const ACTION = "ct-liquid-interactive inline-flex h-7 shrink-0 items-center gap-1.5 rounded-md border px-2 text-[11px] hover:bg-[var(--ct-surface)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)]";
const ACTION_STYLE = { borderColor: "var(--ct-border)", color: "var(--ct-fg)" };

export function revealLocalFile(path: string) {
  void openLocalArtifact(path).catch(() => undefined);
}

export const REVEAL_UNAVAILABLE = "Revealing files is available only in the desktop app";

export function ImageFullscreen({ image, anchorRef, onClose }: {
  image: FullscreenImage;
  anchorRef: RefObject<HTMLElement | null>;
  onClose: () => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  useFullscreenKeyboard(true, panelRef, onClose);
  const { bounds } = useFullscreenHostBounds(true, anchorRef);
  const path = image.path;
  return createPortal(
    <FullscreenOverlay panelRef={panelRef} label={`Image ${image.name}`} onClose={onClose} style={bounds}>
      <header className="flex min-w-0 items-center gap-2 px-1.5 pb-2 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
        <ImageIcon size={14} aria-hidden className="shrink-0" />
        <span className="shrink-0 font-medium" style={{ color: "var(--ct-fg)" }}>{image.name}</span>
        <span className="min-w-0 flex-1 truncate font-mono text-[11px] opacity-70" title={path}>{path}</span>
        {path && localArtifactOpeningAvailable() && (
          <button type="button" className={ACTION} style={ACTION_STYLE} onClick={() => revealLocalFile(path)} aria-label={`Reveal ${image.name}`} title="Reveal in file manager">
            <FolderOpen size={13} aria-hidden />
            Reveal
          </button>
        )}
        <a className={ACTION} style={ACTION_STYLE} href={image.src} download={image.name} aria-label={`Save ${image.name}`}>
          <Download size={13} aria-hidden />
          Save
        </a>
        <button type="button" className={ACTION} style={ACTION_STYLE} onClick={onClose} aria-label="Close image">
          <X size={13} aria-hidden />
        </button>
      </header>
      <div
        className="flex min-h-0 flex-1 items-center justify-center overflow-auto rounded-md p-4"
        style={{ background: "color-mix(in srgb, var(--ct-bg) 82%, black)" }}
        onClick={onClose}
      >
        <img
          src={image.src}
          alt={image.name}
          className="max-h-full max-w-full rounded-md object-contain shadow-2xl"
          onClick={(event) => event.stopPropagation()}
          draggable={false}
        />
      </div>
    </FullscreenOverlay>,
    document.body,
  );
}
