import type { ViewportBox } from "@codeterm/canvas-camera";
import { useMemo, useRef, useState, type RefObject } from "react";
import { Download, FolderOpen, Image as ImageIcon, X } from "lucide-react";
import { ZoomCanvas } from "./ZoomCanvas";
import { galleryNext, layoutGallery } from "./galleryModel";
import { FullscreenOverlay } from "./FullscreenOverlay";
import { localArtifactOpeningAvailable, openLocalArtifact } from "./localArtifactLoader";

export interface FullscreenImage {
  name: string;
  src: string;
  path?: string;
  error?: string;
}

const ACTION = "ct-liquid-interactive inline-flex h-7 shrink-0 items-center gap-1.5 rounded-md border px-2 text-[11px] hover:bg-[var(--ct-surface)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)]";
const ACTION_STYLE = { borderColor: "var(--ct-border)", color: "var(--ct-fg)" };

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
  const gallery = images?.length ? images : [image];
  const [active, setActive] = useState(Math.max(0, gallery.indexOf(image)));
  const [focused, setFocused] = useState<ViewportBox | null>(null);
  const [sizes, setSizes] = useState<Record<string, { width: number; height: number }>>({});
  const layout = useMemo(() => layoutGallery(gallery.map(item => sizes[item.src] ?? { width: 800, height: 600 })), [images, image, sizes]);
  const selected = gallery[active];
  const path = selected.path;
  const navigate = (direction: number) => {
    const next = galleryNext(active, direction, gallery.length);
    setActive(next); setFocused({ ...layout.items[next] });
  };
  return (
    <FullscreenOverlay panelRef={panelRef} closeButton={false} label={`Image ${selected.name}`} onClose={onClose} returnFocusRef={anchorRef}>
      <header className="flex min-w-0 items-center gap-2 px-1.5 pb-2 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
        <ImageIcon size={14} aria-hidden className="shrink-0" />
        <span className="shrink-0 font-medium" style={{ color: "var(--ct-fg)" }}>{selected.name}</span>
        <span className="min-w-0 flex-1 truncate font-mono text-[11px] opacity-70" title={path}>{path}</span>
        {path && !selected.error && localArtifactOpeningAvailable() && (
          <button type="button" className={ACTION} style={ACTION_STYLE} onClick={() => revealLocalFile(path)} aria-label={`Reveal ${selected.name}`} title="Reveal in file manager">
            <FolderOpen size={13} aria-hidden />
            Reveal
          </button>
        )}
        {selected.src && <a className={ACTION} style={ACTION_STYLE} href={selected.src} download={selected.name} aria-label={`Save ${selected.name}`}>
          <Download size={13} aria-hidden />
          Save
        </a>}
        <button type="button" className={ACTION} style={ACTION_STYLE} onClick={onClose} aria-label="Close image">
          <X size={13} aria-hidden />
        </button>
      </header>
      {gallery.length > 1 && <nav aria-label="Image gallery" className="flex shrink-0 gap-3 p-1">
        <button type="button" aria-label="Previous image" onClick={() => navigate(-1)}>←</button>
        <span aria-live="polite">{active + 1} / {gallery.length}</span>
        <button type="button" aria-label="Next image" onClick={() => navigate(1)}>→</button>
      </nav>}
      <ZoomCanvas bounds={layout.bounds} focus={focused} onNavigate={navigate}>
        {gallery.map((item, index) => <button key={item.src + index} type="button" aria-label={`View ${item.name}`}
          style={{ position: "absolute", left: layout.items[index].x, top: layout.items[index].y, width: layout.items[index].width, height: layout.items[index].height, padding: 0, border: 0, background: "transparent" }}
          onClick={() => { setActive(index); setFocused({ ...layout.items[index] }); }}>
          {item.error ? <span role="status" aria-label={`${item.name} not available`} className="flex h-full w-full items-center justify-center border p-4 text-lg">Image not available</span> : <img src={item.src} alt={item.name} draggable={false} style={{ width: "100%", height: "100%", objectFit: "contain" }}
            onLoad={event => {
              const { naturalWidth: width, naturalHeight: height } = event.currentTarget;
              if (width && height) setSizes(previous => previous[item.src]?.width === width && previous[item.src]?.height === height ? previous : { ...previous, [item.src]: { width, height } });
            }} />}
        </button>)}
      </ZoomCanvas>
    </FullscreenOverlay>
  );
}
