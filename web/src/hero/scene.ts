import { clamp, curvePath, measureChannels, reverseCurve, spring, stepSpring, tangentAt } from './physics';
import type { Channel, Spring } from './physics';
import { opacity, paintLiquid, transform } from './liquid';
import type { Liquid } from './liquid';
import { advanceTransfer, makeTransfer, messages } from './transfer';
import type { Transfer } from './transfer';

type Body = { node: HTMLElement; sheen: HTMLElement; x: Spring; y: Spring; tilt: Spring; energy: Spring };

function select<T extends Element>(root: Element, selector: string): T {
  return root.querySelector<T>(selector)!;
}

export function createMeshScene(desk: HTMLDivElement) {
  const cards = Array.from(desk.querySelectorAll<HTMLElement>('.machine-home'));
  const bodies: Body[] = cards.map((card) => ({ node: select(card, '.machine-body'), sheen: select(card, '.machine-sheen'), x: spring(), y: spring(), tilt: spring(), energy: spring() }));
  const routeNodes = Array.from(desk.querySelectorAll<SVGGElement>('.mesh-channel'));
  const currents = routeNodes.map((node) => select<SVGPathElement>(node, '.channel-current'));
  const threads = routeNodes.map((node) => select<SVGPathElement>(node, '.channel-thread'));
  const liquidNodes: Liquid[] = Array.from(desk.querySelectorAll<SVGGElement>('.liquid-transfer')).map((group) => ({
    group, drop: select(group, '.liquid-drop'), shell: select(group, '.liquid-drop > path'), glint: select(group, '.liquid-glint'),
    neck: select(group, '.liquid-neck'), ripple: select(group, '.liquid-ripple'), satellite: select(group, '.liquid-satellite'),
    label: select(group, '.liquid-label'), text: select(group, 'text'), width: 0,
  }));
  let channels: Channel[] = [];
  let transfers: Transfer[] = [];
  let mobile = false;
  let time = 0;
  let sequence = 0;
  let deskWidth = 0;

  function impulse(index: number, angle: number, force: number) {
    const body = bodies[index];
    body.x.velocity += Math.cos(angle) * force;
    body.y.velocity += Math.sin(angle) * force;
    body.tilt.velocity += Math.cos(angle) * force * .17;
    body.energy.velocity += Math.abs(force) * .25;
  }

  function setMessage(index: number, transfer: Transfer) {
    const liquid = liquidNodes[index];
    const kind = transfer.report ? 'report' : 'task';
    liquid.group.dataset.kind = kind;
    liquid.text.textContent = messages[transfer.message][transfer.report ? 1 : 0];
    liquid.width = liquid.text.getComputedTextLength() + (mobile ? 22 : 28);
    routeNodes[index].style.color = transfer.report ? 'var(--ct-magenta)' : 'var(--ct-accent)';
  }

  function resize() {
    mobile = window.matchMedia('(max-width: 650px)').matches;
    deskWidth = desk.clientWidth;
    channels = measureChannels(cards, mobile, window.matchMedia('(max-width: 900px)').matches);
    if (channels.length !== transfers.length) {
      time = 0;
      transfers = channels.map((_, index) => makeTransfer(index));
    }
    routeNodes.forEach((node, index) => {
      const channel = channels[index];
      node.style.display = channel ? '' : 'none';
      if (!channel) {
        opacity(liquidNodes[index].group, 0);
        return;
      }
      setMessage(index, transfers[index]);
      node.querySelectorAll('path').forEach((path) => path.setAttribute('d', curvePath(channel.curve)));
      currents[index].setAttribute('stroke-dasharray', `28 ${channel.length + 28}`);
      const ports = node.querySelectorAll('circle');
      [channel.curve.start, channel.curve.end].forEach((point, i) => transform(ports[i], `translate(${point.x} ${point.y})`));
    });
  }

  function paintBodies(dt: number) {
    bodies.forEach((body) => {
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
    if (liquid.text.textContent !== messages[transfer.message][transfer.report ? 1 : 0]) setMessage(index, transfer);
    const location = transfer.report ? 1 - clamp(transfer.progress.value) : clamp(transfer.progress.value);
    currents[index].setAttribute('stroke-dashoffset', String(-location * channel.length + 14));
    paintLiquid(liquid, transfer.report ? reverseCurve(channel.curve) : channel.curve, transfer, mobile, deskWidth);
  }

  function launchTransfers() {
    let available = (mobile ? 1 : 2) - transfers.filter((transfer) => transfer.active).length;
    const waiting = transfers.map((transfer, index) => ({ transfer, index })).filter(({ transfer }) => !transfer.active)
      .sort((a, b) => Number(b.transfer.report) - Number(a.transfer.report) || a.transfer.next - b.transfer.next);
    for (const { transfer, index } of waiting) {
      if (available <= 0) break;
      if (time < transfer.next) {
        if (transfer.report) break;
        continue;
      }
      available--;
      transfer.active = true;
      setMessage(index, transfer);
      const channel = channels[index];
      const curve = transfer.report ? reverseCurve(channel.curve) : channel.curve;
      impulse(transfer.report ? channel.to : channel.from, tangentAt(curve, 0), -22);
    }
  }

  function render(dt: number) {
    time += dt;
    launchTransfers();
    transfers.forEach((transfer, index) => {
      const channel = channels[index];
      const curve = transfer.report ? reverseCurve(channel.curve) : channel.curve;
      if (transfer.active) {
        advanceTransfer(transfer, dt);
        if (transfer.progress.value >= 1 && transfer.landed < 0) {
          transfer.landed = 0;
          transfer.shape = spring(-.62, -1.8);
          impulse(transfer.report ? channel.from : channel.to, tangentAt(curve, 1), 48);
        }
        if (transfer.landed >= 0) transfer.landed += dt;
        if (transfer.landed > .72) {
          const report = !transfer.report;
          const message = (transfer.message + (report ? 0 : 1)) % messages.length;
          sequence++;
          Object.assign(transfer, makeTransfer(index), { message, report, next: time + (report ? .32 + (index % 2) * .19 : 1.1 + ((sequence * .618) % 1) * 1.7) });
        }
      }
      paintTransfer(index);
    });
    paintBodies(dt);
  }

  function still() {
    liquidNodes.forEach((liquid) => opacity(liquid.group, 0));
    bodies.forEach((body) => {
      body.node.style.transform = '';
      body.node.style.setProperty('--agent-energy', '0');
      body.sheen.style.opacity = '0';
    });
    channels.forEach((channel, index) => {
      opacity(currents[index], 0);
      threads[index].style.strokeWidth = '1';
      if (index !== 0) return;
      const transfer = { ...makeTransfer(index), progress: spring(.5), expansion: spring(1) };
      setMessage(index, transfer);
      opacity(liquidNodes[index].group, 1);
      paintLiquid(liquidNodes[index], channel.curve, transfer, mobile, deskWidth);
    });
  }

  return { resize, render, still };
}
