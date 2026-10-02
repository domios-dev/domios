import { useLayoutEffect, useRef } from 'react';
import { compression, liquidEasing, release, REST, surfaceTension } from './motion';
import type { BrandPose } from './motion';

export type BrandMotion = 'static' | 'interactive' | 'load';

export function useBrandMotion(motion: BrandMotion) {
  const ref = useRef<SVGSVGElement>(null);
  useLayoutEffect(() => {
    const svg = ref.current;
    if (!svg || motion === 'static') return;
    const path = svg.querySelector('path')!;
    const group = svg.querySelector('g')!;
    const control = svg.closest<HTMLElement>('a, button, [tabindex]');
    if (!control) return;
    const preference = matchMedia('(prefers-reduced-motion: reduce)');
    const motionOwner = svg.closest('[data-motion]');
    let ease = (progress: number) => progress;
    let frame = 0;
    let started = 0;
    let mode: 'rest' | 'open' | 'hold' | 'release' = 'rest';
    let heldAmount = 0;
    let releaseAmount = 0;
    let pointer: number | null = null;
    const enabled = () => !preference.matches && !document.hidden && motionOwner?.getAttribute('data-motion') !== 'paused';
    const paint = (pose: BrandPose) => {
      path.setAttribute('d', pose.path);
      if (pose === REST) group.removeAttribute('transform');
      else group.setAttribute('transform', `translate(32 ${40 + pose.y}) scale(${pose.scaleX} ${pose.scaleY}) translate(-32 -40)`);
      svg.dataset.brandState = mode;
    };
    function reset() {
      cancelAnimationFrame(frame);
      frame = 0;
      mode = 'rest';
      pointer = null;
      paint(REST);
    }
    function tick(timestamp: number) {
      frame = 0;
      if (!enabled()) return reset();
      const seconds = Math.max(0, (timestamp - started) / 1000);
      if (mode === 'open') {
        if (seconds >= 1.6) return reset();
        paint(surfaceTension(seconds));
      } else if (mode === 'hold') {
        heldAmount = .12 + .88 * ease(seconds / .16);
        paint(compression(heldAmount));
        if (seconds >= .16) return;
      } else if (mode === 'release') {
        if (seconds >= 1.2) return reset();
        paint(release(seconds, releaseAmount));
      } else return;
      frame = requestAnimationFrame(tick);
    }
    function open() {
      if (!enabled() || mode !== 'rest') return;
      mode = 'open';
      started = performance.now();
      paint(surfaceTension(0));
      frame = requestAnimationFrame(tick);
    }
    function press() {
      if (!enabled()) return;
      ease = liquidEasing(getComputedStyle(path).getPropertyValue('--ct-ease-liquid-settle'));
      cancelAnimationFrame(frame);
      mode = 'hold';
      heldAmount = .12;
      started = performance.now();
      paint(compression(heldAmount));
      frame = requestAnimationFrame(tick);
    }
    function lift() {
      if (mode !== 'hold') return;
      if (!enabled()) return reset();
      cancelAnimationFrame(frame);
      mode = 'release';
      releaseAmount = heldAmount;
      started = performance.now();
      pointer = null;
      frame = requestAnimationFrame(tick);
    }
    const down = (event: PointerEvent) => { if (event.button === 0 && enabled()) { pointer = event.pointerId; press(); } };
    const up = (event: PointerEvent) => { if (pointer === event.pointerId) lift(); };
    const keyDown = (event: KeyboardEvent) => {
      if ((event.code === 'Space' || event.code === 'Enter') && enabled()) {
        event.preventDefault();
        if (!event.repeat) press();
      }
    };
    const keyUp = (event: KeyboardEvent) => {
      if ((event.code === 'Space' || event.code === 'Enter') && mode === 'hold') {
        event.preventDefault();
        lift();
        if (event.code === 'Enter') control.click();
      }
    };
    const sync = () => { if (!enabled()) reset(); };
    const blur = () => { if (mode === 'hold') reset(); };
    const observer = new MutationObserver(sync);
    if (motionOwner) observer.observe(motionOwner, { attributes: true, attributeFilter: ['data-motion'] });
    control.addEventListener('pointerenter', open);
    control.addEventListener('focus', open);
    control.addEventListener('pointerdown', down);
    control.addEventListener('keydown', keyDown);
    control.addEventListener('keyup', keyUp);
    control.addEventListener('blur', blur);
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', reset);
    window.addEventListener('blur', reset);
    document.addEventListener('visibilitychange', sync);
    preference.addEventListener('change', sync);
    if (motion === 'load') open();
    return () => {
      reset();
      observer.disconnect();
      control.removeEventListener('pointerenter', open);
      control.removeEventListener('focus', open);
      control.removeEventListener('pointerdown', down);
      control.removeEventListener('keydown', keyDown);
      control.removeEventListener('keyup', keyUp);
      control.removeEventListener('blur', blur);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('pointercancel', reset);
      window.removeEventListener('blur', reset);
      document.removeEventListener('visibilitychange', sync);
      preference.removeEventListener('change', sync);
    };
  }, [motion]);
  return ref;
}
