export interface MemoryGraphPalette {
  edgeRgb: string;
  labelColor: string;
  clusterEdgeAlpha: number;
  egoEdgeAlpha: number;
  realEdgeAlpha: (confidence: number) => number;
}

export function deriveMemoryGraphPalette(background: string, foreground: string): MemoryGraphPalette {
  const bg = parseCssRgb(background) ?? [13, 17, 23];
  const fg = parseCssRgb(foreground) ?? [201, 209, 217];
  const light = brightness(bg) > 180;

  return {
    edgeRgb: fg.join(","),
    labelColor: foreground,
    clusterEdgeAlpha: light ? 0.3 : 0.18,
    egoEdgeAlpha: light ? 0.62 : 0.55,
    realEdgeAlpha: (confidence: number) => {
      const conf = Math.max(0, Math.min(1, confidence));
      return light ? 0.1 + conf * 0.11 : 0.035 + conf * 0.055;
    },
  };
}

export function readMemoryGraphPalette(root: HTMLElement = document.documentElement): MemoryGraphPalette {
  const style = getComputedStyle(root);
  return deriveMemoryGraphPalette(
    style.getPropertyValue("--ct-bg").trim() || "#0d1117",
    style.getPropertyValue("--ct-fg").trim() || "#c9d1d9",
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
