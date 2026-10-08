import { useCallback, useRef, useState } from "react";
import { Check, Copy, FolderOpen, Maximize2 } from "lucide-react";
import { ImageFullscreen, REVEAL_UNAVAILABLE, revealLocalFile } from "../ImageFullscreen";
import { localArtifactOpeningAvailable } from "../localArtifactLoader";

const FOOTER_ACTION = "ct-liquid-interactive inline-flex h-6 shrink-0 items-center gap-1 rounded-md px-1.5 text-[11px] hover:bg-[var(--ct-surface)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)]";

function CopyPathButton({ path }: { path: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className={FOOTER_ACTION}
      style={{ color: copied ? "var(--ct-green)" : "var(--ct-muted-fg)" }}
      aria-label="Copy image path"
      onClick={() => { void navigator.clipboard?.writeText(path).then(() => setCopied(true), () => undefined); }}
      onMouseLeave={() => setCopied(false)}
      onBlur={() => setCopied(false)}
    >
      {copied ? <Check size={12} aria-hidden /> : <Copy size={12} aria-hidden />}
      {copied ? "Copied" : "Copy"}
    </button>
  );
}

/** An agent-viewed or generated image at reading size; click opens the shared fullscreen viewer. */
export function ImageResultBody({ src, path, localPath, name, alt }: { src: string; path: string; localPath: string; name: string; alt: string }) {
  const [fullscreen, setFullscreen] = useState(false);
  const close = useCallback(() => setFullscreen(false), []);
  const figureRef = useRef<HTMLElement>(null);
  const canReveal = localArtifactOpeningAvailable();
  return (
    <figure ref={figureRef} data-testid="chat-rich-result-figure" className="m-0 mb-1.5 overflow-hidden rounded-lg border" style={{ borderColor: "var(--ct-border)" }}>
      <button
        type="button"
        aria-label={`Open ${alt} fullscreen`}
        onClick={() => setFullscreen(true)}
        className="group relative flex w-full cursor-zoom-in justify-center focus-visible:outline focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-[var(--ct-accent)]"
        style={{ background: "color-mix(in srgb, var(--ct-bg) 82%, black)" }}
      >
        <img data-testid="chat-rich-result-image" src={src} alt={alt} draggable={false} className="block max-h-[28rem] w-auto max-w-full object-contain" />
        <span
          aria-hidden
          className="absolute right-2 top-2 inline-flex h-6 w-6 items-center justify-center rounded-md opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100 motion-safe:transition-opacity"
          style={{ background: "color-mix(in srgb, var(--ct-bg) 72%, transparent)", color: "var(--ct-fg)" }}
        >
          <Maximize2 size={12} />
        </span>
      </button>
      <figcaption className="flex min-w-0 items-center gap-1 border-t py-1 pl-2.5 pr-1" style={{ borderColor: "var(--ct-border)", background: "var(--ct-surface)" }}>
        <span data-field="path" className="min-w-0 flex-1 truncate font-mono text-[11px]" style={{ color: "var(--ct-muted-fg)" }} title={path}>{path}</span>
        <CopyPathButton path={path} />
        <button type="button" className={FOOTER_ACTION} style={{ color: "var(--ct-muted-fg)" }} aria-label={`Reveal ${name}`} title={canReveal ? "Reveal in file manager" : REVEAL_UNAVAILABLE} disabled={!canReveal} onClick={() => revealLocalFile(localPath)}>
          <FolderOpen size={12} aria-hidden />
          Reveal
        </button>
      </figcaption>
      {fullscreen && <ImageFullscreen image={{ name, src, path: localPath }} anchorRef={figureRef} onClose={close} />}
    </figure>
  );
}
