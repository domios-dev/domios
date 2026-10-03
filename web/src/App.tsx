import { AgentNetwork } from './AgentNetwork';
import { Icon } from './Icon';
import { BrandMark } from './BrandMark';
import type { IconName } from './Icon';
import { ProviderStrip } from './ProviderStrip';
import { GatherLink } from './GatherLink';
import { BuilderNotes } from './BuilderNotes';
import { JevSection } from './JevSection';
import { Showcase } from './Showcase';
import { useMediaQuery, useTheme } from './usePreferences';
import { DownloadControl } from './download/DownloadControl';
import { useDownload } from './download/useDownload';
import { DownloadFeedback } from './download/DownloadFeedback';
import { useBrandEntrance } from './brand/useBrandEntrance';

const github = 'https://github.com/rollacode/codeterm-releases';
const pillars: { icon: IconName; title: string; text: string; detail: string }[] = [
  { icon: 'mesh', title: 'One mesh. Your machines.', text: 'Pair your machines into a trusted mesh. Agents reach across it to send commands, hand over work, and return reports.', detail: 'DISTRIBUTED BY DESIGN' },
  { icon: 'terminal', title: 'A place to pick up the thread.', text: 'Agents live in tabs, grouped into project workspaces. Each has a name that stays, a conversation, and a place to return to.', detail: 'TABS & WORKSPACES' },
  { icon: 'grid', title: 'A team shaped for the task.', text: 'The Orchestrator coordinates and verifies. The Planner designs tasks and gives each Worker a fitted personality. Named colleagues, with a purpose.', detail: 'META-TEAMS' },
  { icon: 'branch', title: 'Separate desks. Shared work.', text: 'Workers build in isolated lanes. Accepted work lands through real merges in the main checkout, where it is verified together.', detail: 'ISOLATED LANES' },
  { icon: 'memory', title: 'Context that stays with you.', text: 'Your host keeps durable memory, reusable skills, secrets, and plugins. Agents can change. The environment remains yours. Secrets stay on their machine.', detail: 'MEMORY & SECRETS' },
  { icon: 'layers', title: 'Your choice of intelligence.', text: 'Claude, Codex, OpenCode, Cursor, Grok Build, LM Studio, local models, or a provider plugin you write. One environment for your mix of agents.', detail: 'PROVIDERS & PLUGINS' },
];

export function App() {
  const download = useDownload();
  const { theme, toggle } = useTheme();
  const reducedMotion = useMediaQuery('(prefers-reduced-motion: reduce)');
  const entrance = useBrandEntrance(reducedMotion);
  return <div className="site" data-motion={reducedMotion ? 'paused' : 'playing'} data-entrance={entrance.state}>
    <a className="skip-link" href="#main">Skip to content</a>
    <header className="site-header section-wrap">
      <a className="brand" href="#" aria-label="Domios home"><BrandMark motion="load" onRest={entrance.onRest} />domios</a>
      <nav aria-label="Main navigation"><a href="#explore">Explore</a><a href="#foundation">Why Domios</a><a href={github} className="nav-github">GitHub <span aria-hidden="true">↗</span></a></nav>
      <div className="header-actions"><button className="theme-control" role="switch" aria-checked={theme === 'dark'} aria-label="Dark theme" onClick={toggle}><Icon name="sun" size={15} /><span className="liquid-toggle"><span className="toggle-knob" /></span><Icon name="moon" size={15} /></button><a className="button button-small" href={download.href} data-download-trigger="header" onClick={event => download.onClick(event)} aria-label={download.label}>Download<Icon name="download" size={15} /></a></div>
    </header>
    <main id="main">
      <section className="hero section-wrap" aria-labelledby="hero-title">
        <a className="hero-badge" href="#explore"><BrandMark />THE DISTRIBUTED OS FOR AI AGENTS<span aria-hidden="true">↗</span></a>
        <h1 id="hero-title">Agents <span>live here.</span></h1>
        <ul className="hero-capabilities">
          <li><span>One memory. Every agent.</span><small>Shared on this machine.</small></li>
          <li><span>Scheduled work</span><small>Tasks on your schedule.</small></li>
          <li><span>Your messenger</span><small>Telegram updates &amp; replies</small></li>
        </ul>
        <div className="hero-actions"><DownloadControl download={download} /><a className="button secondary" href={github}><Icon name="github" size={18} /><span>Find us on GitHub</span><Icon name="arrow" size={16} /></a></div>
        <ProviderStrip />
        <AgentNetwork />
      </section>
      <BuilderNotes staticMotion={reducedMotion} />
      <JevSection />
      <Showcase reducedMotion={reducedMotion} />
      <section id="foundation" className="foundation section-wrap" aria-labelledby="foundation-title"><div className="section-heading"><div><p className="eyebrow">THE ENVIRONMENT MATTERS</p><h2 id="foundation-title">Let your agents get to work.</h2></div><p>The infrastructure around the intelligence.<br className="desktop-break" /> Built to keep everything connected.</p></div>
        <div className="pillar-grid">{pillars.map((pillar, index) => <article className="pillar" key={pillar.title}><div className="pillar-top"><Icon name={pillar.icon} size={25} /><span>0{index + 1}</span></div><h3>{pillar.title}</h3><p>{pillar.text}</p><span className="pillar-detail">{pillar.detail}</span></article>)}</div>
        <div className="field-notes">
          <article><span className="eyebrow">WITHIN REACH</span><h3>Leave the desk. Stay in touch.</h3><p>Talk to your agents through Telegram, send voice through a transcription plugin, or open the full interface in a browser over your private Tailnet.</p></article>
          <article><span className="eyebrow">THE ENVIRONMENT AT WORK</span><h3>Keep an eye on the whole.</h3><p>See health, agent usage, and quotas. Check and install signed updates, then return to your work.</p></article>
        </div>
        <aside className="coming-note"><span className="eyebrow">COMING / THE MESH FABRIC</span><p>Shared memory across machines. Project awareness across your mesh. Delegation that finds the right machine for the task.</p></aside>
      </section>
      <section className="closing section-wrap" aria-labelledby="closing-title"><span className="eyebrow">MAKE ROOM FOR WHAT’S NEXT</span><h2 id="closing-title">Your next idea.<br /><span>A whole team behind it.</span></h2><GatherLink data-download-trigger="closing" onClick={event => download.onClick(event)} href={download.href} aria-label={`Get Domios: ${download.label}`}>Get Domios</GatherLink><p>Available for macOS, Windows, and Linux.</p></section>
    </main>
    <footer className="site-footer section-wrap"><a className="brand" href="#" aria-label="Domios home"><BrandMark motion="interactive" />domios</a><span>A place for agents. Built for you.</span><nav aria-label="Footer navigation"><a href={download.href} data-download-trigger="footer" onClick={event => download.onClick(event)} aria-label={download.label}>Download</a><a href={github}>GitHub <span aria-hidden="true">↗</span></a><a href="#explore">Explore</a></nav></footer>
    <DownloadFeedback download={download} />
  </div>;
}
