import { useEffect, useRef } from 'react';
import { clamp } from './hero/physics';
import { advanceStory, createStory, STORY_LEGS, STORY_ROUTES } from './spineStory';
import type { StoryNode, StoryState } from './spineStory';
import { curvePose, storyCurve } from './spineGeometry';
import type { StoryCurve } from './spineGeometry';

function routeFor(from: StoryNode, to: StoryNode) {
  const route = STORY_ROUTES.find((candidate) => candidate.from === from && candidate.to === to
    || candidate.from === to && candidate.to === from);
  return route ? { route, forward: route.from === from } : undefined;
}

function writeState(
  element: HTMLElement,
  state: StoryState,
  nodes: Map<StoryNode, HTMLElement>,
  paths: Map<string, SVGPathElement>,
  cue: HTMLElement,
) {
  const phase = String(state.phase);
  const cycles = String(state.cycles);
  if (element.dataset.storyPhase !== phase) element.dataset.storyPhase = phase;
  if (element.dataset.storyStage !== state.stage) element.dataset.storyStage = state.stage;
  if (element.dataset.storyFrom !== state.from) element.dataset.storyFrom = state.from;
  if (element.dataset.storyTo !== state.to) element.dataset.storyTo = state.to;
  if (element.dataset.storyCycle !== cycles) element.dataset.storyCycle = cycles;
  if (cue.textContent !== STORY_LEGS[state.phase].cue) cue.textContent = STORY_LEGS[state.phase].cue;
  const activeRoute = routeFor(state.from, state.to)?.route.id;
  paths.forEach((path, id) => {
    const active = String(id === activeRoute);
    if (path.dataset.storyActive !== active) path.dataset.storyActive = active;
  });
  nodes.forEach((node, name) => {
    const flow = name === state.from ? 'source' : name === state.to ? 'target' : undefined;
    if (node.dataset.storyFlow !== flow) {
      if (flow) node.dataset.storyFlow = flow;
      else delete node.dataset.storyFlow;
    }
  });
  const evidence = state.to === 'telegram' ? 'active' : 'idle';
  if (element.dataset.storyEvidence !== evidence) element.dataset.storyEvidence = evidence;
}

export function useSpineStory() {
  const boardRef = useRef<HTMLElementTagNameMap['figure']>(null);

  useEffect(() => {
    const element = boardRef.current;
    if (!element) return;
    const board = element;

    const site = board.closest<HTMLElement>('.site');
    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
    const routeElement = board.querySelector<SVGSVGElement>('.jev-routes');
    const signalElement = board.querySelector<SVGEllipseElement>('[data-story-signal]');
    const cueElement = board.querySelector<HTMLElement>('[data-story-cue]');
    if (!routeElement || !signalElement || !cueElement) return;
    const svg = routeElement;
    const signal = signalElement;
    const cue = cueElement;
    const nodes = new Map<StoryNode, HTMLElement>();
    board.querySelectorAll<HTMLElement>('[data-story-node]').forEach((node) => {
      const name = node.dataset.storyNode as StoryNode | undefined;
      if (name) nodes.set(name, node);
    });

    let state = createStory();
    let visible = false;
    let disposed = false;
    let frame = 0;
    let previous = 0;
    let geometryDirty = true;
    const routePoints = new Map<string, StoryCurve>();
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
      STORY_ROUTES.forEach((route) => {
        const from = rects.get(route.from);
        const to = rects.get(route.to);
        const path = paths.get(route.id);
        if (!from || !to || !path) return;
        const points = storyCurve(from, to, bounds, route.kind);
        path.setAttribute('d', `M${points.from.x},${points.from.y}C${points.controlFrom.x},${points.controlFrom.y} ${points.controlTo.x},${points.controlTo.y} ${points.to.x},${points.to.y}`);
        routePoints.set(route.id, points);
      });
      geometryDirty = routePoints.size !== STORY_ROUTES.length;
      return !geometryDirty;
    }

    function paintSignal(current: StoryState) {
      const match = routeFor(current.from, current.to);
      const points = match && routePoints.get(match.route.id);
      if (!match || !points) {
        if (signal.getAttribute('opacity') !== '0') signal.setAttribute('opacity', '0');
        return;
      }

      const progress = current.stage === 'landing' ? 1 : clamp(current.progress);
      const { x, y, angle } = curvePose(points, match.forward ? progress : 1 - progress);
      const stretch = current.stage === 'landing'
        ? clamp(current.landing, -1, 1) * 0.22
        : Math.min(0.32, Math.abs(current.velocity) * 0.12);
      const scaleX = 1 + stretch;
      const scaleY = 1 / Math.sqrt(Math.max(0.7, scaleX));
      signal.setAttribute('transform', `translate(${x} ${y}) rotate(${angle + (match.forward ? 0 : 180)}) scale(${scaleX} ${scaleY})`);
      if (signal.getAttribute('opacity') !== '1') signal.setAttribute('opacity', '1');
    }

    function setStaticDiagram() {
      if (board.dataset.storyMotion !== 'static') board.dataset.storyMotion = 'static';
      if (board.dataset.storyPhase !== 'static') board.dataset.storyPhase = 'static';
      if (board.dataset.storyStage !== 'static') board.dataset.storyStage = 'static';
      if (cue.textContent !== 'Recall · needs input · reply') cue.textContent = 'Recall · needs input · reply';
      if (board.dataset.storyEvidence !== 'static') board.dataset.storyEvidence = 'static';
      paths.forEach((path) => delete path.dataset.storyActive);
      nodes.forEach((node) => delete node.dataset.storyFlow);
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
      writeState(board, state, nodes, paths, cue);
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
      writeState(board, state, nodes, paths, cue);
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
