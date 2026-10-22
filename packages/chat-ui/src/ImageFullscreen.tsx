import { useRef, type RefObject } from "react";
import { Download, FolderOpen, Image as ImageIcon, X } from "lucide-react";
import { ZoomCanvas } from "./ZoomCanvas";
import { useImageCanvasGallery } from "./useImageCanvasGallery";
import { ViewerButton, VIEWER_ACTION, VIEWER_ACTION_STYLE } from "./ViewerButton";
import { DeferredLoader } from "./DeferredLoader";
import { FullscreenOverlay } from "./FullscreenOverlay";
import { localArtifactOpeningAvailable, openLocalArtifact } from "./localArtifactLoader";

export interface FullscreenImage {
  name: string;
  src: string;
  path?: string;
  error?: string;
  pending?: boolean;
}

export function revealLocalFile(path: string) {
  void openLocalArtifact(path).catch(() => undefined);
}

export const REVEAL_UNAVAILABLE = "Revealing files is available only in the desktop app";

export function ImageFullscreen({ image, images, anchorRef, onClose }: {
  images?: FullscreenImage[];
  image: FullscreenImage;
  anchorRef: RefObject<HTMLElement | null>;
  onClose: () => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  const { gallery, active, selected, layout, focus, select, measure, navigate } = useImageCanvasGallery(image, images);
  const path = selected.path;
  return (
    <FullscreenOverlay panelRef={panelRef} closeButton={false} label={`Image ${selected.name}`} onClose={onClose} returnFocusRef={anchorRef}>
      <header className="flex min-w-0 flex-wrap items-center gap-2 px-1.5 pb-2 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
        <ImageIcon size={14} aria-hidden className="shrink-0" />
        <span className="min-w-0 max-w-[40%] truncate font-medium" style={{ color: "var(--ct-fg)" }}>{selected.name}</span>
        <span className="min-w-0 flex-1 truncate font-mono text-[11px] opacity-70" title={path}>{path}</span>
        {path && !selected.error && localArtifactOpeningAvailable() && (
          <ViewerButton onClick={() => revealLocalFile(path)} aria-label={`Reveal ${selected.name}`} title="Reveal in file manager">
            <FolderOpen size={13} aria-hidden />
            Reveal
          </ViewerButton>
        )}
        {selected.src && <a className={VIEWER_ACTION} style={VIEWER_ACTION_STYLE} href={selected.src} download={selected.name} aria-label={`Save ${selected.name}`}>
          <Download size={13} aria-hidden />
          Save
        </a>}
        <ViewerButton onClick={onClose} aria-label="Close image">
          <X size={13} aria-hidden />
        </ViewerButton>
      </header>
      {gallery.length > 1 && <nav aria-label="Image gallery" className="flex shrink-0 gap-3 p-1">
        <ViewerButton aria-label="Previous image" onClick={() => navigate(-1)}>←</ViewerButton>
        <span aria-live="polite">{active + 1} / {gallery.length}</span>
        <ViewerButton aria-label="Next image" onClick={() => navigate(1)}>→</ViewerButton>
      </nav>}
      <ZoomCanvas bounds={layout.bounds} focus={focus} onNavigate={navigate} onSelect={select}>
        {gallery.map((item, index) => <button key={index} type="button" data-zoom-target={index} aria-label={`View ${item.name}`} className="focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]"
          style={{ position: "absolute", left: layout.items[index].x, top: layout.items[index].y, width: layout.items[index].width, height: layout.items[index].height, padding: 0, border: 0, background: "transparent" }}
          onClick={() => select(index)}>
          {item.pending ? <DeferredLoader state="present" role="status" aria-label={`Loading ${item.name}`}>Loading…</DeferredLoader> : item.error ? <span role="status" aria-label={`${item.name} not available`} className="flex h-full w-full items-center justify-center border p-4 text-lg">Image not available</span> : <img src={item.src} alt={item.name} draggable={false} style={{ width: "100%", height: "100%", objectFit: "contain" }}
            onLoad={event => {
              const { naturalWidth: width, naturalHeight: height } = event.currentTarget;
              measure(index, width, height);
            }} />}
        </button>)}
      </ZoomCanvas>
    </FullscreenOverlay>
  );
}
