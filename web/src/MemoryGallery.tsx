import './memory-gallery.css';

export function MemoryGallery() {
  return <section className="memory-gallery section-wrap" aria-label="Memory and Dreams">
    <figure className="memory-feature">
      <div className="memory-feature-image"><img src="/features/memory.webp" width="1200" height="750" alt="Domios Memory graph connecting saved knowledge" loading="lazy" decoding="async" /></div>
      <figcaption><h3>Memory</h3><p>Facts, decisions and skills. One memory for every agent on this machine.</p></figcaption>
    </figure>
    <figure className="memory-feature">
      <div className="memory-feature-image"><img src="/features/dreams.webp" width="1200" height="750" alt="Domios Dreams diary showing memory consolidation sessions" loading="lazy" decoding="async" /></div>
      <figcaption><h3>Dreams</h3><p>Memory revisited and connected. A diary of changes you can review and restore.</p></figcaption>
    </figure>
  </section>;
}
