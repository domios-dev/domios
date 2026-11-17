import { useRef, type RefObject } from "react";
import { Download, Image as ImageIcon, X } from "lucide-react";
import { ZoomCanvas } from "./ZoomCanvas";
import { useImageCanvasGallery } from "./useImageCanvasGallery";
import { CanvasHeader } from "./CanvasHeader";
import { CANVAS_GHOST_BUTTON } from "./CanvasToolbar";
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

export function ImageFullscreen({ image, images, anchorRef, onClose, onImageError }: {
  images?: FullscreenImage[];
  image: FullscreenImage;
  anchorRef: RefObject<HTMLElement | null>;
  onClose: () => void;
  onImageError?: () => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  const { gallery, active, selected, layout, cameraKey, initialFit, focus, select, measure, navigate } = useImageCanvasGallery(image, images);
  const path = selected.path;
  return (
    <FullscreenOverlay panelRef={panelRef} closeButton={false} label={`Image ${selected.name}`} onClose={onClose} returnFocusRef={anchorRef}>
      <div className="-m-2 flex min-h-0 flex-1 flex-col">
      <ZoomCanvas bounds={layout.bounds} cameraKey={cameraKey} initialFit={initialFit} focus={focus} onNavigate={navigate} onSelect={select}
        toolbar="header" renderHeader={controls => <CanvasHeader
          title={<><ImageIcon size={14} aria-hidden className="shrink-0 @max-[560px]:hidden" /><span title={path} aria-label={`Image ${selected.name}`} className="min-w-0 truncate font-medium text-[var(--ct-fg)]">{selected.name}</span></>}
          navigator={gallery.length > 1 && <nav aria-label="Image gallery" className="flex shrink-0 items-center gap-0.5 @max-[560px]:gap-0">
            <button type="button" className={CANVAS_GHOST_BUTTON} aria-label="Previous image" onClick={() => navigate(-1)}>‹</button>
            <span aria-live="polite" className="text-center tabular-nums" style={{ minWidth: `${String(gallery.length).length * 2 + 3}ch` }}>{active + 1} / {gallery.length}</span>
            <button type="button" className={CANVAS_GHOST_BUTTON} aria-label="Next image" onClick={() => navigate(1)}>›</button>
          </nav>}
          view={controls}
          actions={<>
            {path && !selected.error && localArtifactOpeningAvailable() && <button type="button" className={CANVAS_GHOST_BUTTON} onClick={() => revealLocalFile(path)} aria-label={`Reveal ${selected.name}`} title="Reveal in file manager">Reveal</button>}
            {selected.src && <a className={CANVAS_GHOST_BUTTON} href={selected.src} download={selected.name} aria-label={`Save ${selected.name}`} title={`Save ${selected.name}`}><Download size={13} aria-hidden /><span className="@max-[560px]:hidden">Save</span></a>}
          </>}
          close={<button type="button" className={CANVAS_GHOST_BUTTON} onClick={onClose} aria-label="Close image"><X size={13} aria-hidden /></button>}
        />}>
        {gallery.map((item, index) => <button key={index} type="button" data-zoom-target={index} aria-label={`View ${item.name}`} className="focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]"
          style={{ position: "absolute", left: layout.items[index].x, top: layout.items[index].y, width: layout.items[index].width, height: layout.items[index].height, padding: 0, border: 0, background: "transparent" }}
          onClick={() => select(index)}>
          {item.pending ? <DeferredLoader state="present" role="status" aria-label={`Loading ${item.name}`}>Loading…</DeferredLoader> : item.error ? <span role="status" aria-label={`${item.name} not available`} className="flex h-full w-full items-center justify-center border p-4 text-lg">Image not available</span> : <img src={item.src} alt={item.name} draggable={false} style={{ width: "100%", height: "100%", objectFit: "contain" }}
            onError={() => { measure(index, 800, 600); onImageError?.(); }}
            onLoad={event => {
              const { naturalWidth: width, naturalHeight: height } = event.currentTarget;
              measure(index, width, height);
            }} />}
        </button>)}
      </ZoomCanvas>
      </div>
    </FullscreenOverlay>
  );
}
