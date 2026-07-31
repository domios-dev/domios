import {
  MERMAID_FULLSCREEN_MAX_SCALE,
  MERMAID_FULLSCREEN_MIN_SCALE,
} from "./MermaidBlock";

export interface MermaidSize {
  width: number;
  height: number;
}

function positiveSize(width: number, height: number): MermaidSize | null {
  return Number.isFinite(width) && Number.isFinite(height) && width > 0 && height > 0
    ? { width, height }
    : null;
}

export function mermaidSvgSize(svg: string | null): MermaidSize | null {
  if (!svg) return null;
  const viewBox = /\bviewBox\s*=\s*["']\s*([-+\d.e]+)[ ,]+([-+\d.e]+)[ ,]+([-+\d.e]+)[ ,]+([-+\d.e]+)\s*["']/i.exec(svg);
  if (viewBox) {
    const size = positiveSize(Number(viewBox[3]), Number(viewBox[4]));
    if (size) return size;
  }
  const width = /\bwidth\s*=\s*["']\s*([\d.]+)(?:px)?\s*["']/i.exec(svg);
  const height = /\bheight\s*=\s*["']\s*([\d.]+)(?:px)?\s*["']/i.exec(svg);
  return positiveSize(Number(width?.[1]), Number(height?.[1]));
}

export function fitMermaidScale(
  viewport: MermaidSize,
  diagram: MermaidSize,
  padding = 32,
): number {
  const availableWidth = Math.max(1, viewport.width - padding);
  const availableHeight = Math.max(1, viewport.height - padding);
  const scale = Math.min(availableWidth / diagram.width, availableHeight / diagram.height);
  return Math.min(MERMAID_FULLSCREEN_MAX_SCALE, Math.max(MERMAID_FULLSCREEN_MIN_SCALE, scale));
}
