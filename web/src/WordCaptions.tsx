import type { Caption } from './media';

export function WordCaptions({ caption, time }: { caption?: Caption; time: number }) {
  const words = caption?.words;
  const next = words?.findIndex(word => word.start > time) ?? -1;
  const current = next < 0 ? (words?.length ?? 0) - 1 : next - 1;
  const start = Math.max(0, Math.floor(current / 6) * 6);
  const group = words?.slice(start, start + 6);
  return <div className="caption-region" aria-hidden="true">
    {caption && <p className="caption-text" key={`${caption.start}-${start}`}>
      {group ? group.map((word, index) => <span key={start + index} className="caption-word" data-visible={word.start <= time}>{word.text}{index < group.length - 1 ? ' ' : ''}</span>) : caption.text}
    </p>}
  </div>;
}
