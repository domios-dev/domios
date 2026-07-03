import React, { useMemo } from "react";
import { Download } from "lucide-react";
import { PlainOutput } from "./MarkdownBody";
import {
  type ExtractedToolImage,
  parseToolOutputImages,
} from "./toolImageOutput";

function ToolImagePreview({ image, index }: { image: ExtractedToolImage; index: number }) {
  const ext = image.mimeType.replace("image/", "") || "png";
  return (
    <div
      data-testid="chat-tool-image-preview"
      className="rounded-md border overflow-hidden"
      style={{
        borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 20%, transparent)",
        background: "color-mix(in srgb, var(--ct-bg, #0d1117) 72%, black 10%)",
      }}
    >
      <img
        src={image.dataUrl}
        alt={`Tool output image ${index + 1}`}
        className="block max-w-full max-h-64 w-auto object-contain"
      />
      <div className="flex items-center gap-2 px-2 py-1.5">
        <a
          href={image.dataUrl}
          download={`tool-image-${index + 1}.${ext}`}
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
  const parsed = useMemo(() => parseToolOutputImages(text), [text]);
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
