export const THRESHOLD = 'M8 8H32C46 8 56 18 56 32S46 56 32 56H24L40 40V24H24V40L8 56Z';

export type BrandPose = { path: string; scaleX: number; scaleY: number; y: number };
export const REST: BrandPose = { path: THRESHOLD, scaleX: 1, scaleY: 1, y: 0 };

function spring(seconds: number) {
  const decay = 11;
  const frequency = Math.sqrt(420 - decay * decay);
  return 1 - Math.exp(-decay * seconds) * (Math.cos(frequency * seconds) + decay / frequency * Math.sin(frequency * seconds));
}

function threshold(open: number, softness: number) {
  const tip = 8 + 16 * open;
  const right = 24 + 16 * open;
  const top = 40 - 16 * open;
  return `M8 8H32C46 8 56 18 56 32S46 56 32 56H${tip}C${tip + (right - tip) / 3 - softness} ${50.666 - softness} ${tip + (right - tip) * 2 / 3 + softness} 45.333 ${right} 40V${top}H24V40C${18.666 + softness} 45.333 ${13.333 + softness} ${50.666 - softness} 8 56Z`;
}

export function surfaceTension(seconds: number): BrandPose {
  if (seconds >= 1.6) return REST;
  const opening = .12 + .88 * spring(seconds);
  const tension = Math.exp(-5 * seconds) * Math.sin(11 * seconds);
  return { path: threshold(opening, 5 * tension), scaleX: 1 + .055 * tension, scaleY: 1 - .045 * tension, y: 0 };
}

export function compression(amount: number): BrandPose {
  return { path: threshold(1 - .06 * amount, 2 * amount), scaleX: 1 + .1 * amount, scaleY: 1 - .12 * amount, y: 2.4 * amount };
}

export function release(seconds: number, amount: number): BrandPose {
  return seconds >= 1.2 ? REST : compression(amount * (1 - spring(seconds)));
}

export function liquidEasing(value: string) {
  const coordinates = value.match(/-?\d*\.?\d+/g)?.map(Number);
  if (!coordinates || coordinates.length !== 4) return (progress: number) => progress;
  const [x1, y1, x2, y2] = coordinates;
  const bezier = (t: number, a: number, b: number) => 3 * (1 - t) ** 2 * t * a + 3 * (1 - t) * t ** 2 * b + t ** 3;
  return (progress: number) => {
    let low = 0, high = 1;
    for (let step = 0; step < 18; step++) {
      const mid = (low + high) / 2;
      if (bezier(mid, x1, x2) < Math.max(0, Math.min(1, progress))) low = mid;
      else high = mid;
    }
    return bezier((low + high) / 2, y1, y2);
  };
}
