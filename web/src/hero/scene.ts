import { clamp, curvePath, measureChannels, pointAt, reverseCurve, spring, stepSpring, tangentAt } from './physics';
import type { Channel, Curve, Spring } from './physics';

type Transfer = { progress: Spring; shape: Spring; swell: Spring; landed: number; next: number; report: boolean; active: boolean };
type Body = { node: HTMLElement; sheen: HTMLElement; x: Spring; y: Spring; tilt: Spring; energy: Spring };
type Liquid = { group: SVGGElement; drop: SVGGElement; neck: SVGPathElement; ripple: SVGCircleElement; satellite: SVGCircleElement; label: SVGGElement; text: SVGTextElement };

function select<T extends Element>(root: Element, selector: string): T {
  return root.querySelector<T>(selector)!;
}

function makeTransfer(index: number): Transfer {
  return { progress: spring(0, .35), shape: spring(), swell: spring(), landed: -1, next: [.05, .8, .35, 1.45][index], report: false, active: false };
}

function transform(element: Element, value: string) {
  element.setAttribute('transform', value);
}

function opacity(element: Element, value: number) {
  const next = clamp(value).toFixed(3);
  if (element.getAttribute('opacity') !== next) element.setAttribute('opacity', next);
}

function resetSpring(state: Spring) {
  state.value = 0;
  state.velocity = 0;
}

function paintLiquid(liquid: Liquid, curve: Curve, transfer: Transfer, mobile: boolean) {
  const t = clamp(transfer.progress.value);
  const position = pointAt(curve, t);
  const angle = tangentAt(curve, t);
  const landed = transfer.landed >= 0;
  const absorb = landed ? clamp(1 - transfer.landed / .5) : 1;
  const size = (mobile ? .8 : 1) * (landed ? Math.sqrt(absorb) : .7 + .3 * clamp(t / .1));
  const stretch = landed ? 1 + transfer.shape.value : 1 + Math.min(1, Math.abs(transfer.progress.velocity) * .26);
  transform(liquid.drop, `translate(${position.x} ${position.y}) rotate(${angle * 180 / Math.PI}) scale(${Math.max(.25, stretch) * size} ${size / Math.sqrt(Math.max(.3, stretch))})`);
  opacity(liquid.drop, absorb);
  const satellite = pointAt(curve, clamp(t - .07));
  transform(liquid.satellite, `translate(${satellite.x} ${satellite.y}) scale(${size * .85})`);
  opacity(liquid.satellite, !landed && t > .12 && t < .88 ? .5 : 0);
  const source = curve.start;
  const distance = Math.hypot(position.x - source.x, position.y - source.y);
  const neckWidth = 5 * clamp(1 - distance / 48);
  const nx = -Math.sin(angle);
  const ny = Math.cos(angle);
  const midpoint = { x: (source.x + position.x) / 2, y: (source.y + position.y) / 2 };
  if (distance < 48 && !landed) liquid.neck.setAttribute('d', `M${source.x + nx * 7},${source.y + ny * 7}Q${midpoint.x + nx * neckWidth},${midpoint.y + ny * neckWidth} ${position.x + nx * 5},${position.y + ny * 5}L${position.x - nx * 5},${position.y - ny * 5}Q${midpoint.x - nx * neckWidth},${midpoint.y - ny * neckWidth} ${source.x - nx * 7},${source.y - ny * 7}Z`);
  opacity(liquid.neck, distance < 48 && !landed ? clamp(1 - distance / 48) : 0);
  transform(liquid.ripple, `translate(${curve.end.x} ${curve.end.y}) scale(${landed ? 1 + transfer.landed * 5 : 1})`);
  opacity(liquid.ripple, landed ? .65 * clamp(1 - transfer.landed / .65) : 0);
  transform(liquid.label, `translate(${position.x} ${position.y - (mobile ? 23 : 28)})`);
  opacity(liquid.label, landed ? 0 : clamp(t / .12) * clamp((1 - t) / .16));
}

export function createMeshScene(desk: HTMLDivElement) {
  const cards = Array.from(desk.querySelectorAll<HTMLElement>('.machine-home'));
  const bodies: Body[] = cards.map((card) => ({ node: select(card, '.machine-body'), sheen: select(card, '.machine-sheen'), x: spring(), y: spring(), tilt: spring(), energy: spring() }));
  const routeNodes = Array.from(desk.querySelectorAll<SVGGElement>('.mesh-channel'));
  const currents = routeNodes.map((node) => select<SVGPathElement>(node, '.channel-current'));
  const threads = routeNodes.map((node) => select<SVGPathElement>(node, '.channel-thread'));
  const liquidNodes: Liquid[] = Array.from(desk.querySelectorAll<SVGGElement>('.liquid-transfer')).map((group) => ({
    group, drop: select(group, '.liquid-drop'), neck: select(group, '.liquid-neck'), ripple: select(group, '.liquid-ripple'),
    satellite: select(group, '.liquid-satellite'), label: select(group, '.liquid-label'), text: select(group, 'text'),
  }));
  let channels: Channel[] = [];
  let transfers: Transfer[] = [];
  let activeBodyIndices: number[] = [];
  const retiredRoutes = new Set<number>();
  let mobile = false;
  let time = 0;
  let sequence = 0;

  function impulse(index: number, angle: number, force: number) {
    const body = bodies[index];
    body.x.velocity += Math.cos(angle) * force;
    body.y.velocity += Math.sin(angle) * force;
    body.tilt.velocity += Math.cos(angle) * force * .17;
    body.energy.velocity += Math.abs(force) * .25;
  }

  function clearTransferVisual(index: number) {
    const liquid = liquidNodes[index];
    const route = routeNodes[index];
    opacity(liquid.group, 0);
    liquid.group.removeAttribute('data-kind');
    liquid.text.textContent = 'Task';
    [liquid.drop, liquid.satellite, liquid.ripple, liquid.label].forEach((element) => element.removeAttribute('transform'));
    liquid.neck.setAttribute('d', '');
    [liquid.drop, liquid.neck, liquid.ripple, liquid.satellite, liquid.label].forEach((element) => opacity(element, 0));
    opacity(currents[index], 0);
    currents[index].setAttribute('stroke-dashoffset', '0');
    threads[index].style.strokeWidth = '1';
    route.style.removeProperty('color');
  }

  function retireRoute(index: number) {
    const route = routeNodes[index];
    route.style.display = 'none';
    route.querySelectorAll('path').forEach((path) => path.setAttribute('d', ''));
    route.querySelectorAll('circle').forEach((port) => port.removeAttribute('transform'));
    currents[index].setAttribute('stroke-dasharray', '0 0');
    clearTransferVisual(index);
    retiredRoutes.add(index);
  }

  function clearBody(index: number) {
    const body = bodies[index];
    resetSpring(body.x);
    resetSpring(body.y);
    resetSpring(body.tilt);
    resetSpring(body.energy);
    body.node.style.transform = '';
    body.node.style.setProperty('--agent-energy', '0');
    body.sheen.style.opacity = '0';
  }

  function resize() {
    mobile = window.matchMedia('(max-width: 650px)').matches;
    const nextChannels = measureChannels(cards, mobile, window.matchMedia('(max-width: 900px)').matches);
    const nextActiveBodies = [...new Set(nextChannels.flatMap(({ from, to }) => [from, to]))];
    const nextActiveBodySet = new Set(nextActiveBodies);
    activeBodyIndices.filter((index) => !nextActiveBodySet.has(index)).forEach(clearBody);
    activeBodyIndices = nextActiveBodies;
    const topologyChanged = nextChannels.length !== transfers.length;
    const replacingTopology = topologyChanged && transfers.length > 0;
    channels = nextChannels;
    if (topologyChanged) {
      time = 0;
      transfers = channels.map((_, index) => makeTransfer(index));
    }
    routeNodes.forEach((node, index) => {
      const channel = channels[index];
      if (!channel) {
        if (!retiredRoutes.has(index)) retireRoute(index);
        return;
      }
      const wasRetired = retiredRoutes.delete(index);
      node.style.display = '';
      if (replacingTopology && !wasRetired) clearTransferVisual(index);
      node.querySelectorAll('path').forEach((path) => path.setAttribute('d', curvePath(channel.curve)));
      currents[index].setAttribute('stroke-dasharray', `28 ${channel.length + 28}`);
      const ports = node.querySelectorAll('circle');
      [channel.curve.start, channel.curve.end].forEach((point, i) => transform(ports[i], `translate(${point.x} ${point.y})`));
    });
  }

  function paintBodies(dt: number) {
    activeBodyIndices.forEach((index) => {
      const body = bodies[index];
      stepSpring(body.x, 0, 180, 13, dt);
      stepSpring(body.y, 0, 180, 13, dt);
      stepSpring(body.tilt, 0, 150, 12, dt);
      stepSpring(body.energy, 0, 55, 9, dt);
      body.node.style.transform = `translate3d(${body.x.value.toFixed(2)}px,${body.y.value.toFixed(2)}px,0) rotate(${body.tilt.value.toFixed(2)}deg)`;
      body.node.style.setProperty('--agent-energy', clamp(body.energy.value).toFixed(3));
      body.sheen.style.opacity = clamp(body.energy.value * .75).toFixed(3);
    });
  }

  function paintTransfer(index: number) {
    const transfer = transfers[index];
    const channel = channels[index];
    const liquid = liquidNodes[index];
    opacity(liquid.group, transfer.active ? 1 : 0);
    const energy = transfer.active ? clamp(transfer.swell.value, 0, 1.4) * .3 : 0;
    opacity(currents[index], energy);
    threads[index].style.strokeWidth = String(1 + energy * 3);
    if (!transfer.active) return;
    const kind = transfer.report ? 'report' : 'task';
    if (liquid.group.dataset.kind !== kind) {
      liquid.group.dataset.kind = kind;
      liquid.text.textContent = transfer.report ? 'Report' : 'Task';
      routeNodes[index].style.color = transfer.report ? 'var(--ct-magenta)' : 'var(--ct-accent)';
    }
    const location = transfer.report ? 1 - clamp(transfer.progress.value) : clamp(transfer.progress.value);
    currents[index].setAttribute('stroke-dashoffset', String(-location * channel.length + 14));
    paintLiquid(liquid, transfer.report ? reverseCurve(channel.curve) : channel.curve, transfer, mobile);
  }

  function render(dt: number) {
    time += dt;
    transfers.forEach((transfer, index) => {
      const channel = channels[index];
      const curve = transfer.report ? reverseCurve(channel.curve) : channel.curve;
      if (!transfer.active && time >= transfer.next) {
        transfer.active = true;
        impulse(transfer.report ? channel.to : channel.from, tangentAt(curve, 0), -22);
      }
      if (transfer.active) {
        stepSpring(transfer.progress, 1, 32, 5.8, dt);
        stepSpring(transfer.shape, 0, 260, 14, dt);
        stepSpring(transfer.swell, transfer.landed < 0 ? 1 : 0, 120, 10, dt);
        if (transfer.progress.value >= 1 && transfer.landed < 0) {
          transfer.landed = 0;
          transfer.shape = spring(-.62, -1.8);
          impulse(transfer.report ? channel.from : channel.to, tangentAt(curve, 1), 48);
        }
        if (transfer.landed >= 0) transfer.landed += dt;
        if (transfer.landed > .72) {
          const report = !transfer.report;
          sequence++;
          Object.assign(transfer, makeTransfer(index), { report, next: time + (report ? .32 + (index % 2) * .19 : 1.1 + ((sequence * .618) % 1) * 1.7) });
        }
      }
      paintTransfer(index);
    });
    paintBodies(dt);
  }

  function still() {
    liquidNodes.forEach((liquid) => opacity(liquid.group, 0));
    activeBodyIndices.forEach((index) => {
      const body = bodies[index];
      body.node.style.transform = '';
      body.node.style.setProperty('--agent-energy', '0');
      body.sheen.style.opacity = '0';
    });
    channels.forEach((channel, index) => {
      opacity(currents[index], 0);
      threads[index].style.strokeWidth = '1';
      if (index > 1) return;
      const report = index === 1;
      const liquid = liquidNodes[index];
      liquid.group.dataset.kind = report ? 'report' : 'task';
      liquid.text.textContent = report ? 'Report' : 'Task';
      opacity(liquid.group, 1);
      paintLiquid(liquid, report ? reverseCurve(channel.curve) : channel.curve, { ...makeTransfer(index), progress: spring(.52), report }, mobile);
    });
  }

  return { resize, render, still };
}
