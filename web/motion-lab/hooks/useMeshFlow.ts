import { useEffect, useRef, useState } from 'react';

const journeys = [
  { start: 0.9, duration: 2.5 },
  { start: 4.3, duration: 1.8 },
  { start: 7.1, duration: 3.2 },
];

function easeLiquid(value: number) {
  return value < 0.5 ? 4 * value ** 3 : 1 - (-2 * value + 2) ** 3 / 2;
}

export function useMeshFlow(reducedMotion: boolean, paused: boolean, replay: number) {
  const ref = useRef<SVGSVGElement>(null);
  const elapsed = useRef(0);
  const lastReplay = useRef(replay);
  const [phase, setPhase] = useState(0);
  useEffect(() => {
    const svg = ref.current;
    if (!svg) return;
    if (lastReplay.current !== replay) { elapsed.current = 0; lastReplay.current = replay; }
    const paths = Array.from(svg.querySelectorAll<SVGPathElement>('[data-route]'));
    const parcels = Array.from(svg.querySelectorAll<SVGGElement>('[data-parcel]'));
    const lengths = paths.map(path => path.getTotalLength());
    let frame = 0;
    let previous: number | undefined;
    let visible = false;
    let capture = false;
    const paint = (seconds: number) => {
      const nextPhase = seconds >= 10.3 ? 3 : seconds >= 6.1 ? 2 : seconds >= 3.4 ? 1 : 0;
      setPhase(current => current === nextPhase ? current : nextPhase);
      journeys.forEach((journey, index) => {
        const raw = (seconds - journey.start) / journey.duration;
        const parcel = parcels[index];
        if (!parcel) return;
        const active = raw > 0 && raw < 1;
        parcel.style.opacity = active && !reducedMotion ? '1' : '0';
        if (!active) return;
        const progress = easeLiquid(raw);
        const point = paths[index].getPointAtLength(lengths[index] * progress);
        parcel.setAttribute('transform', `translate(${point.x} ${point.y})`);
        const material = parcel.querySelector<SVGElement>('[data-material]');
        const tension = Math.sin(raw * Math.PI * 2);
        const scaleX = 1 + Math.max(0, tension) * 0.32 + Math.min(0, tension) * 0.15;
        material?.setAttribute('transform', `scale(${scaleX} ${1 / Math.sqrt(scaleX)})`);
      });
    };
    const tick = (now: number) => {
      if (previous !== undefined) elapsed.current += (now - previous) / 1000;
      previous = now;
      paint(elapsed.current);
      if (elapsed.current < 11.2) frame = requestAnimationFrame(tick);
    };
    const synchronize = () => {
      cancelAnimationFrame(frame);
      previous = undefined;
      if (reducedMotion) paint(11.2);
      else if (!paused && visible && !document.hidden && !capture && elapsed.current < 11.2) frame = requestAnimationFrame(tick);
    };
    const observer = new IntersectionObserver(entries => { visible = entries[0].isIntersecting; synchronize(); }, { threshold: 0.15 });
    const seek = (event: Event) => {
      capture = true;
      cancelAnimationFrame(frame);
      elapsed.current = (event as CustomEvent<number>).detail;
      paint(reducedMotion ? 11.2 : elapsed.current);
    };
    paint(reducedMotion ? 11.2 : elapsed.current);
    observer.observe(svg);
    document.addEventListener('visibilitychange', synchronize);
    svg.addEventListener('motion-seek', seek);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
      document.removeEventListener('visibilitychange', synchronize);
      svg.removeEventListener('motion-seek', seek);
    };
  }, [reducedMotion, paused, replay]);
  return { ref, phase };
}
