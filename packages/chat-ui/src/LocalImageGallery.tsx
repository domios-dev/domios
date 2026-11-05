import type { CSSProperties, ReactNode } from "react";
import { Image as ImageIcon, ImageOff, RefreshCw, FolderOpen } from "lucide-react";
import { attachmentPathName, type LocalArtifactDirective } from "@codeterm/chat-core";
import { ImageFullscreen, revealLocalFile, REVEAL_UNAVAILABLE } from "./ImageFullscreen";
import { localArtifactOpeningAvailable, type LocalPreviewFile } from "./localArtifactLoader";
import { useLocalImageGallery } from "./useLocalImageGallery";
import { DeferredLoader } from "./DeferredLoader";
import { ViewerButton } from "./ViewerButton";

// Every image is a tile sized to its picture; loading and unavailable keep the loaded row's height
// and one fixed width, so a tile never changes size when its read settles.
const ROW_HEIGHT = 160;
const SINGLE_MAX_HEIGHT = 320;
const PLACEHOLDER = "flex h-40 w-52 flex-col items-center justify-center gap-1.5 px-4 text-center text-[11px] leading-snug";
const FRAME_STYLE: CSSProperties = { borderColor: "var(--ct-border)", background: "color-mix(in srgb, var(--ct-fg) 4%, transparent)", color: "var(--ct-muted-fg)" };
// The tile's own actions stay out of sight until the tile is hovered or focused; touch screens always show them.
const ACTIONS = "absolute right-1.5 top-1.5 flex gap-1 opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100 [@media(hover:none)]:opacity-100";
const ACTION_STYLE: CSSProperties = { minWidth: 28, minHeight: 28, padding: 0, borderColor: "var(--ct-border)", color: "var(--ct-fg)", background: "color-mix(in srgb, var(--ct-surface) 82%, transparent)", backdropFilter: "blur(6px)" };
// A caption never widens its tile: it takes the picture's width and truncates.
const FIT_TILE = "w-0 min-w-full";

export function LocalImageGallery({ directive, fullscreenRequest = 0, bare = false, onLoaded, renderError }: {
  directive: LocalArtifactDirective;
  fullscreenRequest?: number;
  bare?: boolean;
  onLoaded?: (file: LocalPreviewFile | null) => void;
  renderError?: (error: string) => ReactNode;
}) {
  const { images, open, setOpen, retry, anchor, omitted } = useLocalImageGallery(directive, fullscreenRequest, onLoaded);
  const single = !directive.paths || directive.paths.length === 1;
  const reveal = localArtifactOpeningAvailable();
  const imageStyle: CSSProperties = single
    ? { display: "block", maxHeight: SINGLE_MAX_HEIGHT, minHeight: 120,maxWidth: "100%", objectFit: "contain" }
    : { display: "block", height: ROW_HEIGHT, width: "auto", maxWidth: "100%", objectFit: "contain" };
  return <section ref={anchor} data-testid="chat-ds-image" aria-label={single ? `image preview: ${attachmentPathName(directive.path)}` : "Image gallery preview"}
    className={bare ? "" : "my-2"}>
    {omitted > 0 && <p role="alert" className="mb-1.5 text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>Showing the first 32 images. {omitted} additional images were omitted.</p>}
    <div className="flex flex-wrap items-start gap-2">
      {images.map((image, index) => <figure key={index} className="group m-0 inline-flex max-w-full flex-col gap-1">
        <div className="relative max-w-full overflow-hidden rounded-lg border" style={FRAME_STYLE}>
          <button type="button" aria-label={`Fullscreen ${image.name}`} onClick={() => setOpen(index)} className="block max-w-full cursor-zoom-in focus-visible:outline focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-[var(--ct-accent)]" style={{ minWidth: 44, minHeight: 44 }}>
            {image.pending ? <DeferredLoader state="present" role="status" aria-label={`Loading ${image.name}`} className={PLACEHOLDER}><ImageIcon size={18} aria-hidden className="animate-pulse opacity-60" /></DeferredLoader>
              : image.error ? <span role="status" aria-label={`${image.name} not available`} title={image.error} className={PLACEHOLDER}>
                <ImageOff size={18} aria-hidden className="opacity-70" />
                <span style={{ color: "var(--ct-fg)" }}>Image not available</span>
                {!renderError && <span role="alert" className="line-clamp-2 opacity-80">{image.error}</span>}
              </span>
                : <img src={image.src} alt={image.name} loading="lazy" style={imageStyle} />}
          </button>
          {!image.pending && <div className={ACTIONS}>
            <ViewerButton aria-label={`Reveal ${image.name}`} disabled={!reveal || Boolean(image.error)} onClick={() => image.path && revealLocalFile(image.path)} title={reveal ? "Reveal in file manager" : REVEAL_UNAVAILABLE} style={ACTION_STYLE}><FolderOpen size={13} aria-hidden /></ViewerButton>
            <ViewerButton aria-label={`Reload ${image.name}`} onClick={() => retry(index)} title={image.error ? "Try again" : "Reload"} style={ACTION_STYLE}><RefreshCw size={13} aria-hidden /></ViewerButton>
          </div>}
        </div>
        <figcaption className={`${FIT_TILE} truncate px-0.5 text-[11px] leading-4`} style={{ color: "var(--ct-muted-fg)" }} title={image.path}>{image.name}</figcaption>
        {image.error && renderError && <div className={FIT_TILE}>{renderError(image.error)}</div>}
      </figure>)}
    </div>
    {open !== null && images[open] && <ImageFullscreen image={images[open]} images={images} anchorRef={anchor} onClose={() => setOpen(null)} />}
  </section>;
}
