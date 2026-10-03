import { useEffect, useRef, useState } from 'react';

const pixelsPerSecond = 18;

export function useNoteEntrance(staticMotion: boolean) {
  const ref = useRef<HTMLDivElement>(null);
  const [paused, setPaused] = useState(false);
  const staticRef = useRef(staticMotion);
  const updateRef = useRef<() => void>(() => {});
  useEffect(() => {
    const viewport = ref.current;
    const track = viewport?.querySelector<HTMLElement>('.notes-track');
    const list = viewport?.querySelector<HTMLElement>('.notes-list');
    if (!viewport || !track || !list) return;
    const preference = window.matchMedia('(prefers-reduced-motion: reduce)');
    const owner = viewport.closest('[data-motion]');
    let animation: Animation | undefined;
    let frame = 0;
    let width = 0;
    let visible = false;
    let hovering = viewport.matches(':hover');
    let focused = viewport.contains(document.activeElement);
    let browsing = false;
    const disabled = () => staticRef.current || preference.matches || owner?.getAttribute('data-motion') === 'paused';
    const easeRate = (target: number) => {
      cancelAnimationFrame(frame);
      if (!animation) return;
      const active = animation;
      const initial = active.playbackRate;
      const start = performance.now();
      if (target) active.play();
      const tick = (now: number) => {
        const progress = Math.min((now - start) / 360, 1);
        active.updatePlaybackRate(initial + (target - initial) * (1 - (1 - progress) ** 3));
        if (progress < 1) frame = requestAnimationFrame(tick);
        else if (!target) active.pause();
      };
      frame = requestAnimationFrame(tick);
    };
    const update = () => {
      const nextBrowsing = disabled() || focused;
      if (nextBrowsing !== browsing) {
        browsing = nextBrowsing;
        if (browsing) viewport.dataset.browsing = 'true';
        else {
          if (animation && width) animation.currentTime = ((width - viewport.scrollLeft) % width) / pixelsPerSecond * 1000;
          viewport.scrollLeft = 0;
          delete viewport.dataset.browsing;
        }
      }
      if (browsing || !visible || document.hidden) {
        cancelAnimationFrame(frame);
        animation?.updatePlaybackRate(0);
        animation?.pause();
      } else easeRate(hovering ? 0 : 1);
    };
    updateRef.current = update;
    const resize = new ResizeObserver(() => {
      const nextWidth = list.getBoundingClientRect().width;
      if (nextWidth === width) return;
      const phase = animation && width ? Number(animation.currentTime) / (width / pixelsPerSecond * 1000) % 1 : 0;
      animation?.cancel();
      width = nextWidth;
      if (!width) return;
      animation = track.animate([{ transform: `translateX(${-width}px)` }, { transform: 'translateX(0)' }], {
        duration: width / pixelsPerSecond * 1000, iterations: Infinity, easing: 'linear',
      });
      animation.currentTime = phase * width / pixelsPerSecond * 1000;
      animation.pause();
      update();
    });
    const enter = () => { hovering = true; update(); };
    const leave = () => { hovering = false; update(); };
    const focus = (event: FocusEvent) => {
      focused = true;
      update();
      if (event.target instanceof HTMLElement) event.target.scrollIntoView({ block: 'nearest', inline: 'start', behavior: 'instant' });
    };
    const blur = (event: FocusEvent) => {
      if (event.relatedTarget instanceof Node && viewport.contains(event.relatedTarget)) return;
      focused = false;
      update();
    };
    const touch = (event: PointerEvent) => { if (event.pointerType === 'touch') setPaused(true); };
    const wheel = (event: WheelEvent) => { if (Math.abs(event.deltaX) > Math.abs(event.deltaY) || event.shiftKey) setPaused(true); };
    const intersection = new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; update(); });
    const motion = new MutationObserver(update);
    resize.observe(list);
    intersection.observe(viewport);
    if (owner) motion.observe(owner, { attributes: true, attributeFilter: ['data-motion'] });
    preference.addEventListener('change', update);
    document.addEventListener('visibilitychange', update);
    viewport.addEventListener('pointerenter', enter);
    viewport.addEventListener('pointerleave', leave);
    viewport.addEventListener('focusin', focus);
    viewport.addEventListener('focusout', blur);
    viewport.addEventListener('pointerdown', touch);
    viewport.addEventListener('wheel', wheel, { passive: true });
    update();
    return () => {
      cancelAnimationFrame(frame);
      animation?.cancel();
      resize.disconnect();
      intersection.disconnect();
      motion.disconnect();
      preference.removeEventListener('change', update);
      document.removeEventListener('visibilitychange', update);
      viewport.removeEventListener('pointerenter', enter);
      viewport.removeEventListener('pointerleave', leave);
      viewport.removeEventListener('focusin', focus);
      viewport.removeEventListener('focusout', blur);
      viewport.removeEventListener('pointerdown', touch);
      viewport.removeEventListener('wheel', wheel);
      delete viewport.dataset.browsing;
    };
  }, []);
  useEffect(() => {
    staticRef.current = staticMotion || paused;
    updateRef.current();
  }, [staticMotion, paused]);
  return { ref, paused, toggle: () => setPaused(value => !value) };
}
