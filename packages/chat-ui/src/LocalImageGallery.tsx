import { useEffect, useRef, useState } from "react";
import type { LocalArtifactDirective } from "@codeterm/chat-core";
import { ImageFullscreen, type FullscreenImage } from "./ImageFullscreen";
import { loadLocalArtifact, type LocalPreviewFile } from "./localArtifactLoader";
import { useLocalArtifactTabId } from "./LocalArtifactScope";

export function LocalImageGallery({ directive, fullscreenRequest = 0, bare = false, onLoaded }: {
  directive: LocalArtifactDirective;
  fullscreenRequest?: number;
  bare?: boolean;
  onLoaded?: (file: LocalPreviewFile | null) => void;
}) {
  const tabId = useLocalArtifactTabId();
  const [images, setImages] = useState<FullscreenImage[]>([]);
  const [open, setOpen] = useState<number | null>(null);
  const [reload, setReload] = useState(0);
  const anchor = useRef<HTMLElement>(null);
  const loaded = useRef(onLoaded);
  loaded.current = onLoaded;
  const key = JSON.stringify(directive.paths ?? [directive.path]);
  useEffect(() => {
    let cancelled = false;
    setImages([]); loaded.current?.(null);
    const paths = JSON.parse(key) as string[];
    void Promise.all(paths.map(async path => {
      try {
        const file = await loadLocalArtifact(path, "image", tabId);
        return { path, file };
      } catch (error) { return { path, error: String(error) }; }
    })).then(results => {
      if (cancelled) return;
      setImages(results.map(result => ({ path: result.path, name: result.path.split(/[\\/]/).pop() ?? result.path, src: result.file?.data ?? "", error: result.error })));
      loaded.current?.(results.find(result => result.file)?.file ?? null);
    });
    return () => { cancelled = true; };
  }, [key, tabId, reload]);
  useEffect(() => { if (fullscreenRequest) setOpen(0); }, [fullscreenRequest]);
  return <section ref={anchor} data-testid="chat-ds-image" aria-label="Image preview" className="my-2 rounded-lg border p-2" style={{ borderColor: "var(--ct-border)" }}>
    {!bare && <div className="flex items-center gap-2">
      <button type="button" aria-label="Open image gallery" disabled={!images.length} onClick={() => setOpen(0)}>Fullscreen</button>
      <button type="button" aria-label="Reload images" onClick={() => setReload(value => value + 1)}>Reload</button>
    </div>}
    {!images.length && <p role="status">Loading…</p>}
    <div className="flex gap-2 overflow-x-auto">
      {images.map((image, index) => <button key={image.path} type="button" aria-label={`Fullscreen ${image.name}`} onClick={() => setOpen(index)} className="shrink-0">
        {image.error ? <span role="status" aria-label={`${image.name} not available`} className="inline-flex h-32 w-40 items-center justify-center rounded border p-2 text-sm">Image not available</span> : <img src={image.src} alt={image.name} style={{ maxHeight: images.length > 1 ? 160 : 480, maxWidth: "100%", objectFit: "contain" }} />}
      </button>)}
    </div>
    {open !== null && images[open] && <ImageFullscreen image={images[open]} images={images} anchorRef={anchor} onClose={() => setOpen(null)} />}
  </section>;
}
