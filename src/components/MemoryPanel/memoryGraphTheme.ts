export interface MemoryGraphPalette {
  labelColor: string;
  hoverEdgeColor: string;
  selectedEdgeColor: string;
  edgeColor: (strength: number, long?: boolean) => string;
}

export function deriveMemoryGraphPalette(background: string, foreground: string, mutedForeground: string): MemoryGraphPalette {
  const bg = parseCssRgb(background) ?? [13, 17, 23];
  const fg = parseCssRgb(foreground) ?? [201, 209, 217];
  const muted = parseCssRgb(mutedForeground) ?? [139, 148, 158];
  const lightAdjustment = brightness(bg) > 150 ? 0.04 : 0;
  const colors = new Map<string, string>();
  for (const strength of [0.08, 0.1, 0.14, 0.18]) {
    for (const long of [false, true]) {
      colors.set(`${strength}:${long}`, mixRgb(bg, muted, (strength + lightAdjustment) * (long ? 0.5 : 1)));
    }
  }
  return {
    labelColor: foreground,
    hoverEdgeColor: mixRgb(bg, fg, 0.45),
    selectedEdgeColor: mixRgb(bg, fg, 0.65),
    edgeColor: (strength, long = false) => {
      const color = colors.get(`${strength}:${long}`);
      if (!color) throw new Error(`Unsupported memory edge strength: ${strength}`);
      return color;
    },
  };
}

function mixRgb(background: [number, number, number], foreground: [number, number, number], strength: number): string {
  return `rgb(${background.map((channel, index) => Math.round(channel + (foreground[index] - channel) * strength)).join(",")})`;
}

export function readMemoryGraphPalette(root: HTMLElement = document.documentElement): MemoryGraphPalette {
  const style = getComputedStyle(root);
  return deriveMemoryGraphPalette(
    style.getPropertyValue("--ct-bg").trim() || "#0d1117",
    style.getPropertyValue("--ct-fg").trim() || "#c9d1d9",
    style.getPropertyValue("--ct-muted-fg").trim() || "#8b949e",
  );
}

function parseCssRgb(color: string): [number, number, number] | null {
  const c = color.trim();
  const hex = c.match(/^#([0-9a-f]{6})$/i);
  if (hex) {
    const raw = hex[1];
    return [
      parseInt(raw.slice(0, 2), 16),
      parseInt(raw.slice(2, 4), 16),
      parseInt(raw.slice(4, 6), 16),
    ];
  }

  const rgb = c.match(/^rgba?\((\d+),\s*(\d+),\s*(\d+)/i);
  if (rgb) {
    return [
      Number(rgb[1]),
      Number(rgb[2]),
      Number(rgb[3]),
    ];
  }
  return null;
}

function brightness([r, g, b]: [number, number, number]): number {
  return (r * 299 + g * 587 + b * 114) / 1000;
}
