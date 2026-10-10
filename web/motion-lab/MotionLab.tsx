import { useState } from 'react';
import { InhabitedMesh } from './components/InhabitedMesh';
import { WaterlineStudy } from './components/Waterline';
import { LiquidShowcase } from './components/LiquidShowcase';
import { GatherStudy } from './components/GatherButton';
import { useLabPreferences } from './hooks/usePreferences';

const pieces = [
  { id: 'mesh', name: 'Inhabited mesh', label: '01 / THE HERO', title: <>A home here.<br /><em>A team everywhere.</em></>, description: 'A Domios home on every machine. Agents inside. Work flowing between them.' },
  { id: 'waterline', name: 'Waterline', label: '02 / THE PASSAGE', title: <>One thought<br /><em>flows into the next.</em></>, description: 'A seam opens. A new section gathers beneath it. The page keeps its composure.' },
  { id: 'showcase', name: 'Common channel', label: '03 / THE SHOWCASE', title: <>Different views.<br /><em>A common current.</em></>, description: 'A thumb that stretches between places. Captions that settle beside the picture.' },
  { id: 'gather', name: 'Gather', label: '04 / THE SMALL THINGS', title: <>A little pull.<br /><em>A sense of belonging.</em></>, description: 'One detached bead joins the action. A quiet, tactile invitation to come in.' },
];

export function MotionLab() {
  const query = new URLSearchParams(location.search);
  const initial = pieces.findIndex(piece => piece.id === query.get('piece'));
  const [active, setActive] = useState(initial < 0 ? 0 : initial);
  const [paused, setPaused] = useState(false);
  const [replay, setReplay] = useState(0);
  const { reducedMotion, theme, toggleTheme } = useLabPreferences();
  const piece = pieces[active];
  return <div className="motion-lab" data-motion={reducedMotion ? 'reduced' : paused ? 'paused' : 'playing'} data-piece={piece.id}>
    <a href="#study" className="skip-link">Skip to motion study</a>
    <header className="lab-header"><a className="wordmark" href="?piece=mesh" aria-label="Domios motion lab"><svg viewBox="0 0 32 32" aria-hidden="true"><path d="M5 24V14Q5 5 14 5h13v13q0 9-9 9H8" /><path d="M12 21v-6q0-3 3-3h6v6q0 3-3 3h-6Z" /></svg>domios<span>LIQUID FIELD STUDIES</span></a><button className="theme-button" onClick={toggleTheme} role="switch" aria-checked={theme === 'dark'} aria-label="Dark theme"><span className="theme-knob" /><span>{theme === 'dark' ? 'Night ink' : 'Day ink'}</span></button></header>
    <main id="study">
      <div className="lab-layout"><aside className="study-intro"><span className="small-label">{piece.label}</span><h1>{piece.title}</h1><p className="intro-copy">{piece.description}</p><div className="vector-note"><span className="note-mark">≈</span><p>From orchard sluices.<br /><strong>Held by surface tension.</strong></p></div><nav className="study-nav" aria-label="Motion studies">{pieces.map((item, index) => <button key={item.id} onClick={() => setActive(index)} aria-current={active === index ? 'page' : undefined}><span>0{index + 1}</span>{item.name}<i aria-hidden="true">↗</i></button>)}</nav></aside>
      <section className="study-stage" aria-label={piece.name}>
        <div className="stage-topline"><span>DOMIOS / MOTION ATLAS</span><span>{theme === 'dark' ? 'NIGHT' : 'DAY'} EDITION</span></div>
        <div className="study-frame" key={`${active}-${replay}`}>
          {active === 0 && <InhabitedMesh reducedMotion={reducedMotion} paused={paused} />}
          {active === 1 && <WaterlineStudy reducedMotion={reducedMotion} />}
          {active === 2 && <LiquidShowcase />}
          {active === 3 && <GatherStudy />}
        </div>
        <div className="stage-footer"><span>{reducedMotion ? 'Static · reduced motion' : 'Liquid · stretch, arrive, settle'}</span><div><button className="text-button replay-study" onClick={() => setReplay(value => value + 1)} disabled={reducedMotion}>Replay <span aria-hidden="true">↺</span></button><button className="text-button pause-study" aria-pressed={paused} disabled={reducedMotion} onClick={() => setPaused(value => !value)}>{paused ? 'Resume' : 'Pause'} motion</button></div></div>
      </section></div>
    </main>
    <footer className="lab-footer"><span>DOM + OS <i>/</i> MANY HOMES. ONE MESH.</span><span>KINETIC <i>/</i> STUDY 001—004</span></footer>
  </div>;
}
