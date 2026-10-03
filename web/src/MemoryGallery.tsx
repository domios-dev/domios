import './memory-gallery.css';

export function MemoryGallery() {
  return <section className="memory-gallery section-wrap" aria-labelledby="memory-title">
    <h2 id="memory-title">Knowledge that stays.</h2>
    <figure className="memory-feature">
      <a className="memory-feature-image" href="/features/memory.webp" target="_blank" rel="noopener noreferrer" aria-label="Open Memory screenshot in full size"><img src="/features/memory.webp" width="1200" height="863" alt="Domios Memory graph connecting saved knowledge" loading="lazy" decoding="async" /><span>View screenshot ↗</span></a>
      <figcaption><h3>Memory</h3><p>Facts, decisions and skills, shared by agents on this machine.</p></figcaption>
    </figure>
    <figure className="memory-feature">
      <a className="memory-feature-image" href="/features/dreams.webp" target="_blank" rel="noopener noreferrer" aria-label="Open Dreams screenshot in full size"><img src="/features/dreams.webp" width="1200" height="862" alt="Domios Dreams diary showing memory consolidation sessions" loading="lazy" decoding="async" /><span>View screenshot ↗</span></a>
      <figcaption><h3>Dreams</h3><p>Review memory changes and restore saved versions.</p></figcaption>
    </figure>
    <p className="memory-roadmap">On the roadmap: shared memory across machines.</p>
  </section>;
}
