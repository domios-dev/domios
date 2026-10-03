import { useEffect, useRef, useState } from 'react';
import { ProviderLogo } from './ProviderLogo';
import type { ProviderLogoId } from './ProviderLogo';
import { useMediaQuery } from './usePreferences';
import './providers.css';

type ProviderTheme = 'light' | 'dark';
type Provider = { name: string; logo: ProviderLogoId | null; plus?: boolean };

const providers: Provider[] = [
  { name: 'Claude Code', logo: 'claude' },
  { name: 'Codex', logo: 'codex' },
  { name: 'OpenCode', logo: 'opencode' },
  { name: 'Cursor', logo: 'cursor' },
  { name: 'Grok Build', logo: 'grok' },
  { name: 'LM Studio', logo: 'lmstudio' },
  { name: 'Your agent', logo: null, plus: true },
];

function useProviderMarquee() {
  const viewport = useRef<HTMLDivElement>(null);
  const reducedMotion = useMediaQuery('(prefers-reduced-motion: reduce)');
  const [theme, setTheme] = useState<ProviderTheme>(() => document.documentElement.dataset.theme === 'light' ? 'light' : 'dark');

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
      setTheme(document.documentElement.dataset.theme === 'light' ? 'light' : 'dark');
    };

    const intersection = new IntersectionObserver(([entry]) => {
      visible = entry.isIntersecting && entry.intersectionRect.width > 0 && entry.intersectionRect.height > 0;
      sync();
    });
    const motion = new MutationObserver(sync);
    intersection.observe(element);
    if (site) motion.observe(site, { attributes: true, attributeFilter: ['data-motion', 'data-entrance'] });
    motion.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    document.addEventListener('visibilitychange', sync);
    sync();

    return () => {
      intersection.disconnect();
      motion.disconnect();
      document.removeEventListener('visibilitychange', sync);
      strip.dataset.providerMotion = 'paused';
    };
  }, [reducedMotion]);

  return { viewport, reducedMotion, theme };
}

function ProviderEntry({ provider, theme }: { provider: Provider; theme: ProviderTheme }) {
  const wordmark = provider.logo === 'opencode' || provider.logo === 'lmstudio';

  return <>
    {provider.plus ? <span className="provider-neutral-plus" aria-hidden="true">+</span> : provider.logo && <ProviderLogo mark={provider.logo} theme={theme} />}
    <span className={wordmark ? 'provider-name provider-name-visually-hidden' : 'provider-name'}>{provider.name}</span>
  </>;
}

export function ProviderStrip() {
  const { viewport, reducedMotion, theme } = useProviderMarquee();
  return <section className="provider-strip section-wrap" aria-label="Supported agents">
    <div className="provider-viewport" ref={viewport}>
      <div className="provider-track">
        <ul className="provider-list provider-cycle" role="list">{providers.map((provider) => <li className="provider-entry" key={provider.name}><ProviderEntry provider={provider} theme={theme} /></li>)}</ul>
        {!reducedMotion && <div className="provider-cycle provider-copy" aria-hidden="true" inert>{providers.map((provider) => <div className="provider-entry" key={provider.name}><ProviderEntry provider={provider} theme={theme} /></div>)}</div>}
      </div>
    </div>
  </section>;
}
