import { useEffect, useRef } from 'react';
import { createMeshScene } from './scene';

export function useLiquidMesh() {
  const desk = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const element = desk.current;
    if (!element) return;
    const scene = createMeshScene(element);
    const preference = window.matchMedia('(prefers-reduced-motion: reduce)');
    const motionOwner = element.closest('.site') ?? element.closest('[data-motion]');
    let visible = false;
    let frame = 0;
    let previous = 0;
    let disposed = false;

    const staticMotion = () => preference.matches || motionOwner?.getAttribute('data-motion') === 'paused'
      || motionOwner?.getAttribute('data-entrance') === 'pending';
    const canAnimate = () => !disposed && visible && !document.hidden && !staticMotion();

    function tick(timestamp: number) {
      frame = 0;
      if (!canAnimate()) return;
      const elapsed = previous ? Math.min((timestamp - previous) / 1000, 1 / 30) : 0;
      previous = timestamp;
      scene.render(elapsed);
      frame = requestAnimationFrame(tick);
    }

    function sync() {
      if (disposed) return;
      if (canAnimate()) {
        if (!frame) {
          previous = 0;
          frame = requestAnimationFrame(tick);
        }
      } else {
        cancelAnimationFrame(frame);
        frame = 0;
        previous = 0;
        if (staticMotion()) scene.still();
      }
    }

    const intersection = new IntersectionObserver(([entry]) => {
      visible = entry.isIntersecting;
      sync();
    });
    const resize = new ResizeObserver(() => {
      if (disposed) return;
      scene.resize();
      if (staticMotion()) scene.still();
      else if (canAnimate()) scene.render(0);
    });
    const motion = new MutationObserver(sync);
    scene.resize();
    scene.still();
    intersection.observe(element);
    resize.observe(element);
    element.querySelectorAll('.machine-home').forEach((card) => resize.observe(card));
    if (motionOwner) motion.observe(motionOwner, { attributes: true, attributeFilter: ['data-motion', 'data-entrance'] });
    document.addEventListener('visibilitychange', sync);
    preference.addEventListener('change', sync);

    return () => {
      disposed = true;
      cancelAnimationFrame(frame);
      intersection.disconnect();
      resize.disconnect();
      motion.disconnect();
      document.removeEventListener('visibilitychange', sync);
      preference.removeEventListener('change', sync);
    };
  }, []);

  return desk;
}
