import { useEffect, useRef } from 'react';
import { BrandMark } from './BrandMark';
import './jev.css';

function useSpineMotion() {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const site = element.closest('.site');
    const preference = window.matchMedia('(prefers-reduced-motion: reduce)');
    let visible = false;

    const sync = () => {
      const active = visible && !document.hidden && !preference.matches
        && site?.getAttribute('data-motion') !== 'paused'
        && site?.getAttribute('data-entrance') !== 'pending';
      element.dataset.flow = active ? 'running' : 'paused';
    };
    const intersection = new IntersectionObserver(([entry]) => {
      visible = entry.isIntersecting;
      sync();
    });
    const motion = new MutationObserver(sync);
    intersection.observe(element);
    if (site) motion.observe(site, { attributes: true, attributeFilter: ['data-motion', 'data-entrance'] });
    document.addEventListener('visibilitychange', sync);
    preference.addEventListener('change', sync);
    sync();

    return () => {
      intersection.disconnect();
      motion.disconnect();
      document.removeEventListener('visibilitychange', sync);
      preference.removeEventListener('change', sync);
      element.dataset.flow = 'paused';
    };
  }, []);

  return ref;
}

function SpineExchange() {
  return <div className="jev-exchange" role="img" aria-label="Agents send requests to Domios; Domios returns results to agents.">
    <div className="jev-lane jev-lane-out" aria-hidden="true">
      <div className="jev-channel"><i className="jev-arrow" /><div className="jev-flight"><i className="jev-drop" /></div></div>
    </div>
    <div className="jev-lane jev-lane-back" aria-hidden="true">
      <div className="jev-channel"><i className="jev-arrow" /><div className="jev-flight"><i className="jev-drop" /></div></div>
    </div>
  </div>;
}

export function JevSection() {
  const scene = useSpineMotion();
  return <section id="jev" className="jev-section section-wrap" aria-labelledby="jev-title">
    <div className="jev-heading">
      <h2 id="jev-title">Your agents’ home.<br /><span>With a spinal cord.</span></h2>
      <p className="jev-intro">Domios organizes memory, runs schedules and carries work across paired machines, with bounded recovery.</p>
    </div>
    <div className="jev-scene" ref={scene} data-flow="paused">
      <div className="jev-agents">
        <span className="jev-agent-glyphs" aria-hidden="true"><i /><i /><i /></span>
        <strong>Agents</strong>
        <span className="jev-support">Tabs · crons · memory</span>
      </div>
      <SpineExchange />
      <div className="jev-home">
        <BrandMark motion="static" />
        <strong>Domios</strong>
        <span className="jev-home-label">Spinal cord</span>
        <div className="jev-spine" aria-hidden="true"><i /><i /><i /><i /><i /></div>
      </div>
      <div className="jev-socket" aria-hidden="true"><i /><i /></div>
      <div className="jev-engine-group">
        <div className="jev-engine"><strong>JEV</strong><span className="jev-support">Replaceable engine</span></div>
        <span className="jev-support jev-model-choice">Suitable local models, too—via LM Studio adapters.</span>
      </div>
    </div>
    <p className="jev-authority">The model names states. Domios’ deterministic core authorizes actions from evidence.</p>
  </section>;
}
