import React from "react";
import { Download } from "lucide-react";
import { ImageFullscreen } from "./ImageFullscreen";
import { PlainOutput } from "./MarkdownBody";
import {
  type ExtractedToolImage,
  parseToolOutputImages,
} from "./toolImageOutput";

function ToolImagePreview({ image, index }: { image: ExtractedToolImage; index: number }) {
  const ext = image.mimeType.replace("image/", "") || "png";
  const [fullscreen, setFullscreen] = React.useState(false);
  const close = React.useCallback(() => setFullscreen(false), []);
  const frameRef = React.useRef<HTMLDivElement>(null);
  const name = `tool-image-${index + 1}.${ext}`;
  return (
    <div
      ref={frameRef}
      data-testid="chat-tool-image-preview"
      className="rounded-md border overflow-hidden"
      style={{
        borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 20%, transparent)",
        background: "color-mix(in srgb, var(--ct-bg, #0d1117) 72%, black 10%)",
      }}
    >
      <button
        type="button"
        aria-label={`Open tool output image ${index + 1} fullscreen`}
        onClick={() => setFullscreen(true)}
        className="block w-full cursor-zoom-in focus-visible:outline focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-[var(--ct-accent)]"
      >
        <img
          src={image.dataUrl}
          alt={`Tool output image ${index + 1}`}
          className="block max-w-full max-h-[28rem] w-auto object-contain"
          draggable={false}
        />
      </button>
      {fullscreen && <ImageFullscreen image={{ name, src: image.dataUrl }} anchorRef={frameRef} onClose={close} />}
      <div className="flex items-center gap-2 px-2 py-1.5">
        <a
          href={image.dataUrl}
          download={name}
          data-testid="chat-tool-image-download"
          className="inline-flex items-center gap-1 text-[11px] px-1.5 py-0.5 rounded transition-colors hover:brightness-125"
          style={{
            color: "var(--ct-muted-fg)",
            background: "color-mix(in srgb, var(--ct-accent) 15%, transparent)",
          }}
        >
          <Download size={11} strokeWidth={2} />
          Save
        </a>
        <span className="text-[10px] font-mono opacity-50" style={{ color: "var(--ct-muted-fg)" }}>
          {ext.toUpperCase()}
        </span>
      </div>
    </div>
  );
}

export function ToolOutputContent({
  text,
  maxHeight = "12rem",
}: {
  text: string;
  maxHeight?: string;
}) {
  const parsed = React.useMemo(() => parseToolOutputImages(text), [text]);
  if (parsed.images.length === 0) {
    return <PlainOutput text={text} maxHeight={maxHeight} />;
  }
  return (
    <div className="flex flex-col gap-2">
      {parsed.images.map((image, index) => (
        <ToolImagePreview key={`${image.mimeType}-${index}`} image={image} index={index} />
      ))}
      {parsed.remainder && <PlainOutput text={parsed.remainder} maxHeight={maxHeight} />}
    </div>
  );
}
