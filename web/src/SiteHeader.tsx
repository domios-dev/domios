import { useEffect, useRef, useState } from 'react';
import { BrandMark } from './BrandMark';
import { Icon } from './Icon';
import { siteLinks } from './siteLinks';
import type { DownloadSelection } from './download/useDownload';
import './header.css';

export function SiteHeader({ download, github, theme, toggleTheme, onBrandRest }: {
  download: DownloadSelection;
  github: string;
  theme: string;
  toggleTheme: () => void;
  onBrandRest: () => void;
}) {
  const marker = useRef<HTMLSpanElement>(null);
  const [compact, setCompact] = useState(false);
  useEffect(() => {
    const element = marker.current;
    if (!element) return;
    const observer = new IntersectionObserver(([entry]) => {
      setCompact(!entry.isIntersecting && entry.boundingClientRect.top < 0);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return <>
    <span className="header-scroll-marker" ref={marker} aria-hidden="true" />
    <div className="site-header-shell" data-compact={compact}>
      <header className="site-header section-wrap">
        <a className="brand" href="#" aria-label="Domios home"><BrandMark motion="load" onRest={onBrandRest} /><span className="header-brand-name">domios</span></a>
        <nav aria-label="Main navigation"><a href="#explore">Explore</a><a href="#foundation">Why Domios</a></nav>
        <div className="header-actions">
          <a className="button button-small header-social" href={siteLinks.discord} target="_blank" rel="noopener noreferrer" aria-label="Join Domios on Discord" title="Discord"><img src={`/community/discord-${theme === 'dark' ? 'white' : 'black'}.svg`} width="22" height="22" alt="" /></a>
          <a className="button button-small header-social" href={siteLinks.telegram} target="_blank" rel="noopener noreferrer" aria-label="Follow Domios on Telegram" title="Telegram"><img src="/community/telegram.svg" width="22" height="22" alt="" /></a>
          <button className="theme-control" role="switch" aria-checked={theme === 'dark'} aria-label="Dark theme" onClick={toggleTheme}><Icon name="sun" size={15} /><span className="liquid-toggle"><span className="toggle-knob" /></span><Icon name="moon" size={15} /></button>
          <a className="button button-small header-download" href={download.href} data-download-trigger="header" onClick={event => download.onClick(event)} aria-label={download.label} title={download.label}><span className="header-action-label">Download</span><Icon name="download" size={18} /></a>
          <a className="button button-small header-github" href={github} aria-label="Find us on GitHub" title="Find us on GitHub"><Icon name="github" size={18} /><span className="header-action-label">GitHub</span></a>
        </div>
      </header>
    </div>
  </>;
}
