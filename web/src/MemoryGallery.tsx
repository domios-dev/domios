import { useState } from 'react';
import { Icon } from './Icon';
import { ScreenshotDialog } from './ScreenshotDialog';
import './memory-gallery.css';

const screenshots = [
  { title: 'Memory', src: '/features/memory.webp', width: 1200, height: 863, alt: 'Domios Memory graph connecting saved knowledge', description: 'Facts, decisions and skills, shared by agents on this machine.' },
  { title: 'Dreams', src: '/features/dreams.webp', width: 1200, height: 862, alt: 'Domios Dreams diary showing memory consolidation sessions', description: 'Review memory changes and restore saved versions.' },
];

export function MemoryGallery() {
  const [selected, setSelected] = useState<number | null>(null);
  return <section className="memory-gallery section-wrap" aria-labelledby="memory-title">
    <h2 id="memory-title">Knowledge that stays.</h2>
    {screenshots.map((image, index) => <figure className="memory-feature" key={image.title}>
      <button type="button" className="memory-feature-image" aria-label={`View ${image.title} screenshot`} aria-haspopup="dialog" onClick={() => setSelected(index)}><img src={image.src} width={image.width} height={image.height} alt={image.alt} loading="lazy" decoding="async" /><span>View screenshot <Icon name="expand" size={14} /></span></button>
      <figcaption><h3>{image.title}</h3><p>{image.description}</p></figcaption>
    </figure>)}
    <p className="memory-roadmap">On the roadmap: shared memory across machines.</p>
    {selected !== null && <ScreenshotDialog images={screenshots} selected={selected} onSelect={setSelected} onClose={() => setSelected(null)} />}
  </section>;
}
