/** Parse "#rrggbb" → [r, g, b]. */
export function parseHex(hex: string): [number, number, number] {
  const c = hex.replace("#", "");
  return [parseInt(c.substring(0, 2), 16), parseInt(c.substring(2, 4), 16), parseInt(c.substring(4, 6), 16)];
}

/** Blend two hex colors by ratio (0 = all colorA, 1 = all colorB). */
export function blendColor(hexA: string, hexB: string, ratio: number): string {
  const a = parseHex(hexA), b = parseHex(hexB);
  const r = Math.round(a[0] + (b[0] - a[0]) * ratio);
  const g = Math.round(a[1] + (b[1] - a[1]) * ratio);
  const bl = Math.round(a[2] + (b[2] - a[2]) * ratio);
  return `#${r.toString(16).padStart(2, "0")}${g.toString(16).padStart(2, "0")}${bl.toString(16).padStart(2, "0")}`;
}

export function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l * 100];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h = 0;
  if (max === r) h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
  else if (max === g) h = ((b - r) / d + 2) / 6;
  else h = ((r - g) / d + 4) / 6;
  return [h * 360, s * 100, l * 100];
}

export function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  h /= 360; s /= 100; l /= 100;
  if (s === 0) { const v = Math.round(l * 255); return [v, v, v]; }
  const hue2rgb = (p: number, q: number, t: number) => {
    if (t < 0) t += 1; if (t > 1) t -= 1;
    if (t < 1/6) return p + (q - p) * 6 * t;
    if (t < 1/2) return q;
    if (t < 2/3) return p + (q - p) * (2/3 - t) * 6;
    return p;
  };
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  return [
    Math.round(hue2rgb(p, q, h + 1/3) * 255),
    Math.round(hue2rgb(p, q, h) * 255),
    Math.round(hue2rgb(p, q, h - 1/3) * 255),
  ];
}

/** Derive color in HSL space — lightens dark colors, darkens light ones. */
export function deriveColor(hex: string, amount: number): string {
  const [r, g, b] = parseHex(hex);
  const [h, s, l] = rgbToHsl(r, g, b);
  const brightness = (r + g + b) / 3;
  const dir = brightness > 128 ? -1 : 1;
  const nl = Math.min(100, Math.max(0, l + amount * dir * 0.4));
  const [nr, ng, nb] = hslToRgb(h, s, nl);
  return `#${nr.toString(16).padStart(2, "0")}${ng.toString(16).padStart(2, "0")}${nb.toString(16).padStart(2, "0")}`;
}

/** Simple RGB brightness adjust (no hue preservation). */
export function adjustBrightness(hex: string, amount: number): string {
  const [r, g, b] = parseHex(hex);
  const dir = (r + g + b) / 3 > 128 ? -1 : 1;
  const nr = Math.min(255, Math.max(0, r + amount * dir));
  const ng = Math.min(255, Math.max(0, g + amount * dir));
  const nb = Math.min(255, Math.max(0, b + amount * dir));
  return `#${nr.toString(16).padStart(2, "0")}${ng.toString(16).padStart(2, "0")}${nb.toString(16).padStart(2, "0")}`;
}
