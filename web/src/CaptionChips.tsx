import type { Caption } from './media';

export function CaptionChips({ caption }: { caption?: Caption }) {
  return <div className="caption-region" aria-hidden="true">
    {caption && <><span className="caption-anchor" /><p className="caption-chip" key={`${caption.start}-${caption.text}`}>{caption.text}</p></>}
  </div>;
}
