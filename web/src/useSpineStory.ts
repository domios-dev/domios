import { useEffect, useRef } from 'react';
import { clamp } from './hero/physics';
import { advanceStory, createStory } from './spineStory';
import type { StoryNode, StoryState } from './spineStory';

type Point = { x: number; y: number };
type Route = { id: string; from: StoryNode; to: StoryNode };
type RoutePoints = { from: Point; to: Point };

const ROUTES: Route[] = [
  { id: 'team-home', from: 'team', to: 'home' },
  { id: 'home-memory', from: 'home', to: 'memory' },
  { id: 'home-engine', from: 'home', to: 'engine' },
  { id: 'home-telegram', from: 'home', to: 'telegram' },
];

function edgePoint(rect: DOMRect, towardX: number, towardY: number, origin: DOMRect): Point {
  const centerX = rect.left + rect.width / 2;
  const centerY = rect.top + rect.height / 2;
  const dx = towardX - centerX;
  const dy = towardY - centerY;
  const scaleX = Math.abs(dx) > 0.001 ? rect.width / 2 / Math.abs(dx) : Number.POSITIVE_INFINITY;
  const scaleY = Math.abs(dy) > 0.001 ? rect.height / 2 / Math.abs(dy) : Number.POSITIVE_INFINITY;
  const scale = Math.min(scaleX, scaleY);
  return { x: centerX + dx * scale - origin.left, y: centerY + dy * scale - origin.top };
}

function routeFor(from: StoryNode, to: StoryNode): { route: Route; forward: boolean } | undefined {
  const route = ROUTES.find((candidate) => candidate.from === from && candidate.to === to
    || candidate.from === to && candidate.to === from);
  return route ? { route, forward: route.from === from } : undefined;
}

function writeState(element: HTMLElement, state: StoryState) {
  const phase = String(state.phase);
  const cycles = String(state.cycles);
  if (element.dataset.storyPhase !== phase) element.dataset.storyPhase = phase;
  if (element.dataset.storyStage !== state.stage) element.dataset.storyStage = state.stage;
  if (element.dataset.storyFrom !== state.from) element.dataset.storyFrom = state.from;
  if (element.dataset.storyTo !== state.to) element.dataset.storyTo = state.to;
  if (element.dataset.storyCycle !== cycles) element.dataset.storyCycle = cycles;
}

export function useSpineStory() {
  const boardRef = useRef<HTMLElementTagNameMap['figure']>(null);

  useEffect(() => {
    const board = boardRef.current;
    if (!board) return;

    const site = board.closest<HTMLElement>('.site');
    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
    const svg = board.querySelector<SVGSVGElement>('.jev-routes');
    const signal = board.querySelector<SVGEllipseElement>('[data-story-signal]');
    const nodes = new Map<StoryNode, HTMLElement>();
    board.querySelectorAll<HTMLElement>('[data-story-node]').forEach((node) => {
      const name = node.dataset.storyNode as StoryNode | undefined;
      if (name) nodes.set(name, node);
    });
    if (!svg || !signal) return;

    let state = createStory();
    let visible = false;
    let disposed = false;
    let frame = 0;
    let previous = 0;
    let geometryDirty = true;
    const routePoints = new Map<string, RoutePoints>();
    const paths = new Map<string, SVGPathElement>();
    svg.querySelectorAll<SVGPathElement>('[data-story-route]').forEach((path) => {
      const id = path.dataset.storyRoute;
      if (id) paths.set(id, path);
    });

    const motionIsStatic = () => reducedMotion.matches
      || site?.getAttribute('data-motion') === 'paused';
    const entryIsPending = () => site?.getAttribute('data-entrance') === 'pending';
    const canAnimate = () => !disposed && visible && !document.hidden && !motionIsStatic() && !entryIsPending();

    function refreshGeometry() {
      const bounds = svg.getBoundingClientRect();
      if (bounds.width <= 0 || bounds.height <= 0) return false;
      const rects = new Map<StoryNode, DOMRect>();
      nodes.forEach((node, name) => {
        const rect = node.getBoundingClientRect();
        if (rect.width > 0 && rect.height > 0) rects.set(name, rect);
      });
      if (rects.size !== nodes.size) return false;

      svg.setAttribute('viewBox', `0 0 ${bounds.width} ${bounds.height}`);
      routePoints.clear();
      ROUTES.forEach((route) => {
        const from = rects.get(route.from);
        const to = rects.get(route.to);
        const path = paths.get(route.id);
        if (!from || !to || !path) return;
        const start = edgePoint(from, to.left + to.width / 2, to.top + to.height / 2, bounds);
        const end = edgePoint(to, from.left + from.width / 2, from.top + from.height / 2, bounds);
        path.setAttribute('d', `M${start.x},${start.y}L${end.x},${end.y}`);
        routePoints.set(route.id, { from: start, to: end });
      });
      geometryDirty = routePoints.size !== ROUTES.length;
      return !geometryDirty;
    }

    function paintSignal(current: StoryState) {
      const match = routeFor(current.from, current.to);
      const points = match && routePoints.get(match.route.id);
      if (!match || !points) {
        if (signal.getAttribute('opacity') !== '0') signal.setAttribute('opacity', '0');
        return;
      }

      const start = match.forward ? points.from : points.to;
      const end = match.forward ? points.to : points.from;
      const progress = current.stage === 'landing' ? 1 : clamp(current.progress);
      const x = start.x + (end.x - start.x) * progress;
      const y = start.y + (end.y - start.y) * progress;
      const angle = Math.atan2(end.y - start.y, end.x - start.x) * 180 / Math.PI;
      const stretch = current.stage === 'landing'
        ? clamp(current.landing, -1, 1) * 0.22
        : Math.min(0.32, Math.abs(current.velocity) * 0.12);
      const scaleX = 1 + stretch;
      const scaleY = 1 / Math.sqrt(Math.max(0.7, scaleX));
      signal.setAttribute('transform', `translate(${x} ${y}) rotate(${angle}) scale(${scaleX} ${scaleY})`);
      if (signal.getAttribute('opacity') !== '1') signal.setAttribute('opacity', '1');
    }

    function setStaticDiagram() {
      if (board.dataset.storyMotion !== 'static') board.dataset.storyMotion = 'static';
      if (board.dataset.storyPhase !== 'static') board.dataset.storyPhase = 'static';
      if (board.dataset.storyStage !== 'static') board.dataset.storyStage = 'static';
      if (signal.getAttribute('opacity') !== '0') signal.setAttribute('opacity', '0');
      if (signal.hasAttribute('transform')) signal.removeAttribute('transform');
    }

    function tick(timestamp: number) {
      frame = 0;
      if (!canAnimate()) return;
      const elapsed = previous ? Math.min((timestamp - previous) / 1000, 1 / 30) : 0;
      previous = timestamp;
      if (elapsed > 0) state = advanceStory(state, elapsed);
      if (geometryDirty) refreshGeometry();
      writeState(board, state);
      paintSignal(state);
      frame = requestAnimationFrame(tick);
    }

    function sync() {
      if (disposed) return;
      if (motionIsStatic()) {
        if (frame) cancelAnimationFrame(frame);
        frame = 0;
        previous = 0;
        setStaticDiagram();
        if (visible && !document.hidden && geometryDirty) refreshGeometry();
        return;
      }
      if (!canAnimate()) {
        if (frame) cancelAnimationFrame(frame);
        frame = 0;
        previous = 0;
        if (board.dataset.storyMotion !== 'paused') board.dataset.storyMotion = 'paused';
        return;
      }
      if (geometryDirty) refreshGeometry();
      board.dataset.storyMotion = 'running';
      writeState(board, state);
      paintSignal(state);
      if (!frame) {
        previous = 0;
        frame = requestAnimationFrame(tick);
      }
    }

    const intersection = new IntersectionObserver(([entry]) => {
      visible = entry.isIntersecting;
      sync();
    });
    const resize = new ResizeObserver(() => {
      geometryDirty = true;
      if (!visible || document.hidden || disposed) return;
      if (geometryDirty && refreshGeometry() && canAnimate()) paintSignal(state);
    });
    const motion = new MutationObserver(sync);
    intersection.observe(board);
    resize.observe(board);
    nodes.forEach((node) => resize.observe(node));
    if (site) motion.observe(site, { attributes: true, attributeFilter: ['data-motion', 'data-entrance'] });
    document.addEventListener('visibilitychange', sync);
    reducedMotion.addEventListener('change', sync);
    sync();

    return () => {
      disposed = true;
      if (frame) cancelAnimationFrame(frame);
      intersection.disconnect();
      resize.disconnect();
      motion.disconnect();
      document.removeEventListener('visibilitychange', sync);
      reducedMotion.removeEventListener('change', sync);
      if (board.dataset.storyMotion !== 'paused') board.dataset.storyMotion = 'paused';
    };
  }, []);

  return boardRef;
}
