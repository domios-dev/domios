import type { ReactNode } from "react";
import { Image as ImageIcon, RefreshCw, Maximize2, FolderOpen } from "lucide-react";
import { attachmentPathName, type LocalArtifactDirective } from "@codeterm/chat-core";
import { ImageFullscreen, revealLocalFile, REVEAL_UNAVAILABLE } from "./ImageFullscreen";
import { localArtifactOpeningAvailable, type LocalPreviewFile } from "./localArtifactLoader";
import { useLocalImageGallery } from "./useLocalImageGallery";
import { DeferredLoader } from "./DeferredLoader";
import { ViewerButton } from "./ViewerButton";
import { GALLERY_PATH_LIMIT } from "./galleryLoading";

export function LocalImageGallery({ directive, fullscreenRequest = 0, bare = false, onLoaded, renderError }: {
  directive: LocalArtifactDirective;
  fullscreenRequest?: number;
  bare?: boolean;
  onLoaded?: (file: LocalPreviewFile | null) => void;
  renderError?: (error: string) => ReactNode;
}) {
  const { images, open, setOpen, reload, anchor, omitted, loading } = useLocalImageGallery(directive, fullscreenRequest, onLoaded);
  const name = attachmentPathName(directive.path);
  const single = !directive.paths || directive.paths.length === 1;
  const reveal = localArtifactOpeningAvailable();
  return <section ref={anchor} data-testid="chat-ds-image" aria-label={single ? `image preview: ${name}` : "Image gallery preview"}
    className={bare ? "" : "my-2 overflow-hidden rounded-lg border"} style={bare ? undefined : { borderColor: "var(--ct-border)", background: "var(--ct-surface)" }}>
    {!bare && <header className="flex min-w-0 flex-wrap items-center gap-2 border-b px-2.5 py-1.5 text-[11px]" style={{ borderColor: "var(--ct-border)", color: "var(--ct-muted-fg)" }}>
      <ImageIcon size={14} aria-hidden />
      <span className="font-medium" style={{ color: "var(--ct-fg)" }}>{single ? name : `${Math.min(directive.paths?.length ?? 1, GALLERY_PATH_LIMIT)} images`}</span>
      <span className="min-w-0 flex-1 truncate opacity-60" title={directive.path}>{directive.path}</span>
      <ViewerButton aria-label={single ? `Fullscreen ${name}` : "Open image gallery"} disabled={!images.length} onClick={() => setOpen(0)} title="Fullscreen"><Maximize2 size={13} aria-hidden /></ViewerButton>
      {single && <ViewerButton aria-label={`Reveal ${name}`} disabled={!reveal || Boolean(images[0]?.error)} onClick={() => revealLocalFile(directive.path)} title={reveal ? "Reveal in file manager" : REVEAL_UNAVAILABLE}><FolderOpen size={13} aria-hidden /></ViewerButton>}
      <ViewerButton aria-label={single ? `Reload ${name}` : "Reload images"} disabled={loading} onClick={reload} title="Reload"><RefreshCw size={13} aria-hidden /></ViewerButton>
    </header>}
    {omitted > 0 && <p role="alert">Showing the first 32 images. {omitted} additional images were omitted.</p>}
    <div className="flex gap-2 overflow-x-auto p-2">
      {images.map((image, index) => <div key={index} className="shrink-0">
        <button type="button" aria-label={`Fullscreen ${image.name}`} onClick={() => setOpen(index)} className="rounded focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]" style={{ minWidth: 44, minHeight: 44 }}>
          {image.pending ? <DeferredLoader state="present" role="status" aria-label={`Loading ${image.name}`} className="inline-flex h-32 w-40 items-center justify-center">Loading…</DeferredLoader>
            : image.error ? <span role="status" aria-label={`${image.name} not available`} className="inline-flex h-32 w-40 items-center justify-center rounded border p-2 text-sm">Image not available</span>
              : <img src={image.src} alt={image.name} loading="lazy" style={{ maxHeight: single ? 480 : 160, maxWidth: "100%", objectFit: "contain" }} />}
        </button>
        {image.error && (renderError ? renderError(image.error) : <p role="alert" className="max-w-64 whitespace-pre-wrap text-xs">{image.error}</p>)}
      </div>)}
    </div>
    {open !== null && images[open] && <ImageFullscreen image={images[open]} images={images} anchorRef={anchor} onClose={() => setOpen(null)} />}
  </section>;
}
