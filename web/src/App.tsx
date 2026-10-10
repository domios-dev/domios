import { AgentNetwork } from './AgentNetwork';
import { BrandMark } from './BrandMark';
import { ProviderStrip } from './ProviderStrip';
import { BuilderNotes } from './BuilderNotes';
import { JevSection } from './JevSection';
import { Showcase } from './Showcase';
import { useMediaQuery, useTheme } from './usePreferences';
import { DownloadActions } from './download/DownloadActions';
import { useDownload } from './download/useDownload';
import { DownloadFeedback } from './download/DownloadFeedback';
import { useBrandEntrance } from './brand/useBrandEntrance';
import { SiteHeader } from './SiteHeader';
import { MemoryGallery } from './MemoryGallery';
import { BuiltIn } from './BuiltIn';
import { siteLinks } from './siteLinks';

const github = siteLinks.repository;

export function App() {
  const download = useDownload();
  const { theme, toggle } = useTheme();
  const reducedMotion = useMediaQuery('(prefers-reduced-motion: reduce)');
  const entrance = useBrandEntrance(reducedMotion);
  return <div className="site" data-motion={reducedMotion ? 'paused' : 'playing'} data-entrance={entrance.state}>
    <a className="skip-link" href="#main">Skip to content</a>
    <SiteHeader download={download} github={github} theme={theme} toggleTheme={toggle} onBrandRest={entrance.onRest} />
    <main id="main">
      <section className="hero section-wrap" aria-labelledby="hero-title">
        <h1 id="hero-title">Agents <span>live here.</span></h1>
        <DownloadActions download={download} github={github} />
        <AgentNetwork />
        <ProviderStrip />
        <p className="provider-hint">Ask your agent to build a plugin with Domios.<br />Publish it from your repo and share it.</p>
      </section>
      <Showcase reducedMotion={reducedMotion} />
      <JevSection />
      <MemoryGallery />
      <BuiltIn />
      <BuilderNotes staticMotion={reducedMotion} />
      <section className="closing section-wrap" aria-labelledby="closing-title"><h2 id="closing-title">Your next idea.<br /><span>A whole team behind it.</span></h2><DownloadActions download={download} github={github} placement="closing" /><p>Available for macOS, Windows, and Linux.</p></section>
    </main>
    <footer className="site-footer section-wrap"><a className="brand" href="#" aria-label="Domios home"><BrandMark motion="interactive" />domios</a><span>Agents live here</span><nav aria-label="Footer navigation"><a href={download.href} data-download-trigger="footer" onClick={event => download.onClick(event)} aria-label={download.label}>Download</a><a href={github}>GitHub <span aria-hidden="true">↗</span></a><a href="#explore">Explore</a></nav></footer>
    <DownloadFeedback download={download} />
  </div>;
}
