import { useRef, useState, type RefObject } from "react";
import { Download, FolderOpen, Image as ImageIcon, X } from "lucide-react";
import { ZoomCanvas } from "./ZoomCanvas";
import { useImageCanvasGallery } from "./useImageCanvasGallery";
import { CanvasHeader } from "./CanvasHeader";
import { CANVAS_GHOST_BUTTON } from "./CanvasToolbar";
import { DeferredLoader } from "./DeferredLoader";
import { FullscreenOverlay } from "./FullscreenOverlay";
import { loadLocalArtifact, localArtifactOpeningAvailable, openLocalArtifact } from "./localArtifactLoader";
import { useLocalArtifactTabId } from "./LocalArtifactScope";

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

function SaveImage({ name, src, path, saving, onSave }: FullscreenImage & { saving: boolean; onSave: (path: string, name: string) => void }) {
  const label = `Save ${name}`;
  const contents = <><Download size={13} aria-hidden /><span className="@max-[560px]:hidden">Save</span></>;
  if (!path) return <a className={CANVAS_GHOST_BUTTON} href={src} download={name} aria-label={label} title={label}>{contents}</a>;

  return <button type="button" className={CANVAS_GHOST_BUTTON} disabled={saving} aria-label={label} title={label} onClick={() => onSave(path, name)}>{contents}</button>;
}

export function ImageFullscreen({ image, images, anchorRef, onClose, onImageError }: {
  images?: FullscreenImage[];
  image: FullscreenImage;
  anchorRef: RefObject<HTMLElement | null>;
  onClose: () => void;
  onImageError?: () => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  const tabId = useLocalArtifactTabId();
  const { gallery, active, selected, layout, cameraKey, initialFit, focus, select, measure, navigate } = useImageCanvasGallery(image, images);
  const path = selected.path;
  const [saveAttempts, setSaveAttempts] = useState<ReadonlyMap<string, "saving" | "failed">>(() => new Map());
  const saveStatus = path ? saveAttempts.get(path) : undefined;

  async function save(sourcePath: string, name: string) {
    setSaveAttempts(current => new Map(current).set(sourcePath, "saving"));
    try {
      const file = await loadLocalArtifact(sourcePath, "image-original", tabId);
      if (!file.data.startsWith(`data:${file.media_type};base64,`)) throw new Error("Original image unavailable");
      const link = document.createElement("a");
      link.href = file.data;
      link.download = name;
      link.click();
      setSaveAttempts(current => {
        const next = new Map(current);
        next.delete(sourcePath);
        return next;
      });
    } catch {
      setSaveAttempts(current => new Map(current).set(sourcePath, "failed"));
    }
  }

  return (
    <FullscreenOverlay panelRef={panelRef} closeButton={false} label={`Image ${selected.name}`} onClose={onClose} returnFocusRef={anchorRef}>
      <div className="-m-2 flex min-h-0 flex-1 flex-col">
      <ZoomCanvas bounds={layout.bounds} cameraKey={cameraKey} initialFit={initialFit} focus={focus} onNavigate={navigate} onSelect={select}
        toolbar="header" renderHeader={controls => <><CanvasHeader
          title={<><ImageIcon size={14} aria-hidden className="shrink-0 @max-[560px]:hidden" /><span title={path} aria-label={`Image ${selected.name}`} className="min-w-0 truncate font-medium text-[var(--ct-fg)]">{selected.name}</span></>}
          navigator={gallery.length > 1 && <nav aria-label="Image gallery" className="flex shrink-0 items-center gap-0.5 @max-[560px]:gap-0">
            <button type="button" className={CANVAS_GHOST_BUTTON} aria-label="Previous image" onClick={() => navigate(-1)}>‹</button>
            <span aria-live="polite" className="text-center tabular-nums" style={{ minWidth: `${String(gallery.length).length * 2 + 3}ch` }}>{active + 1} / {gallery.length}</span>
            <button type="button" className={CANVAS_GHOST_BUTTON} aria-label="Next image" onClick={() => navigate(1)}>›</button>
          </nav>}
          view={controls}
          actions={<>
            {path && !selected.error && localArtifactOpeningAvailable(tabId) && <button type="button" className={CANVAS_GHOST_BUTTON} onClick={() => revealLocalFile(path)} aria-label={`Reveal ${selected.name}`} title="Reveal in file manager"><FolderOpen size={13} aria-hidden /><span className="@max-[560px]:hidden">Reveal</span></button>}
            {selected.src && <SaveImage key={path ?? active} {...selected} saving={saveStatus === "saving"} onSave={save} />}
          </>}
          close={<button type="button" className={CANVAS_GHOST_BUTTON} onClick={onClose} aria-label="Close image"><X size={13} aria-hidden /></button>}
        />
          {saveStatus === "failed" && <p role="alert" className="m-0 shrink-0 px-3 py-1 text-xs text-[var(--ct-muted-fg)]">Couldn't save the original image.</p>}
        </>}>
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
