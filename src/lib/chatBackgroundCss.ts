import type { ChatIcon } from "../../gen/ChatIcon";
import { CHAT_DOODLES } from "./chatDoodles";
import type { ThemeConfig } from "./types";

const DEFAULT_SCALE = 20;
const DOODLE_SCALE = 360;
export const CHAT_BACKGROUND_DEFAULT_OPACITY = { light: 0.035, dark: 0.028 } as const;
const DOODLE_OPACITY = { light: 0.09, dark: 0.085 };
const MAX_OPACITY = 0.24;
const GRADIENT_OPACITY = 0.08;
const hexColor = /^#[0-9a-f]{6}$/i;

const glyphs: Record<ChatIcon, string> = {
  cat: '<path d="M5 10V4l5 4h4l5-4v6a8 8 0 1 1-14 0Z"/><path d="M9 13h.01m6 0h.01m-4 3 1 1 1-1"/>',
  diamond: '<path d="m12 3 9 9-9 9-9-9Z"/>',
  star: '<path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1 6.2-5.5-2.9-5.5 2.9 1-6.2L3 9.6l6.2-.9Z"/>',
};

export function chatBackgroundToCSS(theme: ThemeConfig, light: boolean): string {
  const background = theme.chat_background;
  const pattern = background?.pattern ?? "none";
  const isDoodles = pattern === "doodles";
  const scale = bounded(background?.scale, 8, 512, isDoodles ? DOODLE_SCALE : DEFAULT_SCALE);
  const angle = bounded(background?.rotation, -360, 360, 0);
  const defaultOpacity = (isDoodles ? DOODLE_OPACITY : CHAT_BACKGROUND_DEFAULT_OPACITY)[light ? "light" : "dark"];
  const opacity = bounded(background?.opacity, 0, MAX_OPACITY, defaultOpacity);
  const foreground = hexColor.test(theme.foreground) ? theme.foreground : (light ? "#1f2328" : "#c9d1d9");
  const customColor = background?.color && hexColor.test(background.color) ? background.color : undefined;
  const color = customColor ?? foreground;
  const layers: string[] = [];

  if (opacity > 0) {
    switch (pattern) {
      case "dots": {
        const ink = customColor ?? "var(--ct-fg)";
        const offset = scale / 2;
        layers.push(`radial-gradient(circle, ${translucent(ink, opacity)} 0.75px, transparent 1.1px) ${offset}px ${offset}px / ${scale}px ${scale}px repeat`);
        break;
      }
      case "grid": {
        const line = translucent(color, opacity / 2);
        for (const direction of [angle, angle + 90]) {
          layers.push(`repeating-linear-gradient(${direction}deg, ${line} 0 1px, transparent 1px ${scale}px)`);
        }
        break;
      }
      case "icon": {
        const glyph = glyphs[background?.icon ?? "cat"];
        if (typeof glyph === "string") {
          layers.push(svgTile(
            `<svg x="25%" y="25%" width="50%" height="50%" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">${glyph}</svg>`,
            scale, color, opacity, angle,
          ));
        }
        break;
      }
      case "doodles":
      case "svg": {
        const art = isDoodles ? CHAT_DOODLES : background?.svg;
        if (typeof art === "string") layers.push(svgTile(art, scale, color, opacity, angle));
        break;
      }
    }
  }

  const gradient = background?.gradient;
  if (gradient && Array.isArray(gradient.stops) && gradient.stops.length >= 2
    && gradient.stops.length <= 4 && gradient.stops.every(stop => hexColor.test(stop))) {
    const stops = gradient.stops.map(stop => translucent(stop, GRADIENT_OPACITY)).join(", ");
    if (gradient.kind === "linear") {
      layers.push(`linear-gradient(${bounded(gradient.angle, -360, 360, 135)}deg, ${stops}) no-repeat`);
    } else if (gradient.kind === "radial") {
      layers.push(`radial-gradient(ellipse at center, ${stops}) no-repeat`);
    }
  }
  return layers.join(", ") || "none";
}

function translucent(color: string, opacity: number): string {
  const percentage = Number((opacity * 100).toFixed(4));
  return `color-mix(in srgb, ${color} ${percentage}%, transparent)`;
}

function svgTile(art: string, scale: number, color: string, opacity: number, angle: number): string {
  const middle = scale / 2;
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${scale} ${scale}"><g color="${color}" fill="${color}" opacity="${opacity}" transform="rotate(${angle} ${middle} ${middle})">${art}</g></svg>`;
  return `url("data:image/svg+xml,${encodeURIComponent(svg)}") 0 0 / ${scale}px ${scale}px repeat`;
}

export function chatPatternTile(background: NonNullable<ThemeConfig["chat_background"]>, scale: number): string | undefined {
  let art = background.svg;
  if (background.pattern === "doodles") art = CHAT_DOODLES;
  if (background.pattern === "icon") {
    const glyph = glyphs[background.icon ?? "cat"];
    if (typeof glyph !== "string") return undefined;
    art = `<svg x="25%" y="25%" width="50%" height="50%" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">${glyph}</svg>`;
  }
  if (!art) return undefined;
  const middle = scale / 2;
  const angle = bounded(background.rotation, -360, 360, 0);
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${scale} ${scale}"><g transform="rotate(${angle} ${middle} ${middle})">${art}</g></svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

function bounded(value: number | undefined, min: number, max: number, fallback: number): number {
  return value !== undefined && Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : fallback;
}
