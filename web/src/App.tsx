import { useState } from 'react';
import { AgentNetwork } from './AgentNetwork';
import { BrandMark, Icon } from './Icon';
import type { IconName } from './Icon';
import { Showcase } from './Showcase';
import { useMediaQuery, useTheme } from './usePreferences';

const download = 'https://github.com/rollacode/codeterm-releases/releases/latest';
const github = 'https://github.com/rollacode/codeterm-releases';
const pillars: { icon: IconName; title: string; text: string; detail: string }[] = [
  { icon: 'terminal', title: 'A home for every agent.', text: 'Give each agent a tab, a workspace, and a purpose. Move between conversations without losing the thread.', detail: 'TABS & WORKSPACES' },
  { icon: 'grid', title: 'A team with a plan.', text: 'Orchestrators coordinate. Planners shape the work. Workers execute with a clear role and a task of their own.', detail: 'META-TEAMS' },
  { icon: 'branch', title: 'Room to work in parallel.', text: 'Agents build in isolated lanes. Work comes together through real merges, with verification where it lands.', detail: 'ISOLATED LANES' },
  { icon: 'mesh', title: 'Across your machines.', text: 'Connect a trusted mesh. Reach agents and workspaces on another machine from the one in front of you.', detail: 'DISTRIBUTED BY DESIGN' },
  { icon: 'memory', title: 'Context that stays.', text: 'Durable memory and reusable skills live with the host. Secrets stay on their machine, under your control.', detail: 'MEMORY & SECRETS' },
  { icon: 'layers', title: 'Your choice of intelligence.', text: 'Claude, Codex, OpenCode, and local models. Extend your environment with providers and capability plugins.', detail: 'PROVIDERS & PLUGINS' },
];

export function App() {
  const { theme, toggle } = useTheme();
  const reducedMotion = useMediaQuery('(prefers-reduced-motion: reduce)');
  const [motionPaused, setMotionPaused] = useState(false);
  return <div className="site" data-motion={reducedMotion || motionPaused ? 'paused' : 'playing'}>
    <a className="skip-link" href="#main">Skip to content</a>
    <header className="site-header section-wrap">
      <a className="brand" href="#" aria-label="Domios home"><BrandMark />domios</a>
      <nav aria-label="Main navigation"><a href="#explore">Explore</a><a href="#foundation">Why Domios</a><a href={github} className="nav-github">GitHub <span aria-hidden="true">↗</span></a></nav>
      <div className="header-actions"><button className="theme-control" role="switch" aria-checked={theme === 'dark'} aria-label="Dark theme" onClick={toggle}><Icon name="sun" size={15} /><span className="liquid-toggle"><span className="toggle-knob" /></span><Icon name="moon" size={15} /></button><a className="button button-small" href={download}>Download<Icon name="download" size={15} /></a></div>
    </header>
    <main id="main">
      <section className="hero section-wrap" aria-labelledby="hero-title">
        <a className="hero-badge" href="#explore"><span className="status-dot" />THE OS FOR AI AGENTS<span aria-hidden="true">↗</span></a>
        <h1 id="hero-title">Agents <span>live here.</span></h1>
        <p className="hero-copy">A home for your agents. A workspace for their work.<br className="desktop-break" /> Bring your teams, tools, and machines together.</p>
        <div className="hero-actions"><a className="button primary" href={download}>Download Domios<Icon name="download" size={18} /></a><a className="button secondary" href={github}><Icon name="github" size={18} />Find us on GitHub<Icon name="arrow" size={16} /></a></div>
        <p className="platforms">macOS <span>·</span> Windows <span>·</span> Linux <span className="platform-divider">/</span> Your machine. Your agents.</p>
        <AgentNetwork />
        {!reducedMotion && <button className="motion-control" aria-pressed={motionPaused} onClick={() => setMotionPaused(!motionPaused)}><Icon name={motionPaused ? 'play' : 'pause'} size={12} />{motionPaused ? 'Resume' : 'Pause'} animation</button>}
      </section>
      <div className="provider-strip section-wrap"><span>BRING YOUR AGENTS</span><div><span className="provider-claude">✳</span> Claude</div><div><Icon name="terminal" size={20} /> Codex</div><div><span className="provider-open">⌘</span> OpenCode</div><div><Icon name="layers" size={20} /> Local models</div></div>
      <Showcase reducedMotion={reducedMotion} />
      <section id="foundation" className="foundation section-wrap" aria-labelledby="foundation-title"><div className="section-heading"><div><p className="eyebrow">THE ENVIRONMENT MATTERS</p><h2 id="foundation-title">Let your agents get to work.</h2></div><p>The infrastructure around the intelligence.<br className="desktop-break" /> Built to keep everything connected.</p></div>
        <div className="pillar-grid">{pillars.map((pillar, index) => <article className="pillar" key={pillar.title}><div className="pillar-top"><Icon name={pillar.icon} size={25} /><span>0{index + 1}</span></div><h3>{pillar.title}</h3><p>{pillar.text}</p><span className="pillar-detail">{pillar.detail}</span></article>)}</div>
      </section>
      <section className="closing section-wrap" aria-labelledby="closing-title"><span className="eyebrow">MAKE ROOM FOR WHAT’S NEXT</span><h2 id="closing-title">Your next idea.<br /><span>A whole team behind it.</span></h2><a className="button primary" href={download}>Get Domios<Icon name="arrow" size={18} /></a><p>Available for macOS, Windows, and Linux.</p></section>
    </main>
    <footer className="site-footer section-wrap"><a className="brand" href="#" aria-label="Domios home"><BrandMark />domios</a><span>A place for agents. Built for you.</span><nav aria-label="Footer navigation"><a href={download}>Download</a><a href={github}>GitHub <span aria-hidden="true">↗</span></a><a href="#explore">Explore</a></nav></footer>
  </div>;
}
