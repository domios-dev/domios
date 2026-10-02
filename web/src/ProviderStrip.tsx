import { useEffect, useRef } from 'react';
import { Icon } from './Icon';
import type { IconName } from './Icon';
import { useMediaQuery } from './usePreferences';
import './providers.css';

const providers: { name: string; icon: IconName }[] = [
  { name: 'Claude Code', icon: 'branch' },
  { name: 'Codex', icon: 'terminal' },
  { name: 'OpenCode', icon: 'grid' },
  { name: 'Cursor', icon: 'arrow' },
  { name: 'Grok Build', icon: 'mesh' },
  { name: 'LM Studio', icon: 'layers' },
  { name: 'Your agent', icon: 'plus' },
];

function useProviderMarquee() {
  const viewport = useRef<HTMLDivElement>(null);
  const reducedMotion = useMediaQuery('(prefers-reduced-motion: reduce)');

  useEffect(() => {
    const element = viewport.current;
    const strip = element?.closest<HTMLElement>('.provider-strip');
    if (!element || !strip) return;
    const site = element.closest('.site');
    let visible = false;

    const sync = () => {
      const staticMotion = reducedMotion || site?.getAttribute('data-motion') === 'paused';
      const canAnimate = visible && !document.hidden && site?.getAttribute('data-entrance') !== 'pending';
      strip.dataset.providerMotion = staticMotion ? 'static' : canAnimate ? 'running' : 'paused';
    };

    const intersection = new IntersectionObserver(([entry]) => {
      visible = entry.isIntersecting && entry.intersectionRect.width > 0 && entry.intersectionRect.height > 0;
      sync();
    });
    const motion = new MutationObserver(sync);
    intersection.observe(element);
    if (site) motion.observe(site, { attributes: true, attributeFilter: ['data-motion', 'data-entrance'] });
    document.addEventListener('visibilitychange', sync);
    sync();

    return () => {
      intersection.disconnect();
      motion.disconnect();
      document.removeEventListener('visibilitychange', sync);
      strip.dataset.providerMotion = 'paused';
    };
  }, [reducedMotion]);

  return { viewport, reducedMotion };
}

function ProviderEntry({ name, icon }: { name: string; icon: IconName }) {
  return <><Icon name={icon} size={20} /><span>{name}</span></>;
}

export function ProviderStrip() {
  const { viewport, reducedMotion } = useProviderMarquee();
  return <section className="provider-strip section-wrap" aria-labelledby="providers-title">
    <h2 id="providers-title">BRING YOUR AGENTS</h2>
    <div className="provider-viewport" ref={viewport}>
      <div className="provider-track">
        <ul className="provider-list provider-cycle" role="list">{providers.map((provider) => <li className="provider-entry" key={provider.name}><ProviderEntry {...provider} /></li>)}</ul>
        {!reducedMotion && <div className="provider-cycle provider-copy" aria-hidden="true" inert>{providers.map((provider) => <div className="provider-entry" key={provider.name}><ProviderEntry {...provider} /></div>)}</div>}
      </div>
    </div>
    <p>Add other agents with provider plugins—ask your agent to write one and share it.</p>
  </section>;
}
