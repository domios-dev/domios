import { clamp, pointAt, tangentAt } from './physics';
import type { Curve } from './physics';
import type { Transfer } from './transfer';

export type Liquid = {
  group: SVGGElement; drop: SVGGElement; shell: SVGPathElement; glint: SVGEllipseElement;
  neck: SVGPathElement; ripple: SVGCircleElement; satellite: SVGCircleElement;
  label: SVGGElement; text: SVGTextElement; width: number;
};

export function transform(element: Element, value: string) {
  element.setAttribute('transform', value);
}

export function opacity(element: Element, value: number) {
  const next = clamp(value).toFixed(3);
  if (element.getAttribute('opacity') !== next) element.setAttribute('opacity', next);
}

function messageShell(expansion: number, width: number, wobble: number, mobile: boolean) {
  if (expansion < .001) return 'M-12 0C-12-7-6-11 1-10C9-10 13-5 13 0S9 10 1 10C-6 11-12 7-12 0Z';
  const w = 12 + (width / 2 - 12) * expansion;
  const h = 10 + ((mobile ? 15 : 17) - 10) * expansion;
  const r = Math.min(w, h);
  const sway = wobble * expansion;
  return `M${-w},0C${-w},${-h * .6} ${-w + r * .4},${-h + sway} ${-w + r},${-h + sway}L${w - r},${-h - sway}C${w - r * .4},${-h - sway} ${w},${-h * .6} ${w},0S${w - r * .4},${h + sway} ${w - r},${h + sway}L${-w + r},${h - sway}C${-w + r * .4},${h - sway} ${-w},${h * .6} ${-w},0Z`;
}

export function paintLiquid(liquid: Liquid, curve: Curve, transfer: Transfer, mobile: boolean, deskWidth: number) {
  const t = clamp(transfer.progress.value);
  const position = pointAt(curve, t);
  const angle = tangentAt(curve, t);
  const expansion = clamp(transfer.expansion.value, 0, 1.04);
  const wave = Math.sin(transfer.reading * 3.6);
  const halfWidth = liquid.width / 2 + 3;
  position.x += (clamp(position.x, halfWidth, deskWidth - halfWidth) - position.x) * expansion;
  position.y += wave * 1.7 * expansion;
  const landed = transfer.landed >= 0;
  const absorb = landed ? clamp(1 - transfer.landed / .5) : 1;
  const dropletSize = (mobile ? .8 : 1) * (landed ? Math.sqrt(absorb) : .7 + .3 * clamp(t / .1));
  const size = dropletSize + (1 - dropletSize) * expansion;
  const stretch = landed ? 1 + transfer.shape.value : 1 + Math.min(1, Math.abs(transfer.progress.velocity) * .26) * (1 - expansion);
  const shellAngle = expansion > .001 ? Math.atan(Math.tan(angle)) : angle;
  transform(liquid.drop, `translate(${position.x} ${position.y}) rotate(${shellAngle * 180 / Math.PI * (1 - clamp(expansion))}) scale(${Math.max(.25, stretch) * size} ${size / Math.sqrt(Math.max(.3, stretch))})`);
  liquid.shell.setAttribute('d', messageShell(expansion, liquid.width, wave * .7, mobile));
  opacity(liquid.drop, absorb);
  opacity(liquid.glint, .42 * (1 - clamp(expansion)));
  const satellite = pointAt(curve, clamp(t - .07));
  transform(liquid.satellite, `translate(${satellite.x} ${satellite.y}) scale(${size * .85})`);
  opacity(liquid.satellite, !landed && t > .12 && t < .88 ? .5 * (1 - clamp(expansion)) : 0);
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
  transform(liquid.label, `translate(${position.x} ${position.y})`);
  opacity(liquid.label, landed ? 0 : clamp((expansion - .92) / .08));
}
