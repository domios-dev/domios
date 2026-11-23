import type { ChatBackground } from "../../gen/ChatBackground";
import type { ChatPattern } from "../../gen/ChatPattern";
import type { ChatWallpaper } from "../../gen/ChatWallpaper";
import type { WallpaperFill } from "../../gen/WallpaperFill";
import type { WallpaperFillPreset } from "../../gen/WallpaperFillPreset";
import type { WallpaperPalette } from "../../gen/WallpaperPalette";
import { CHAT_BACKGROUND_DEFAULT_OPACITY, chatBackgroundToCSS, chatPatternTile } from "./chatBackgroundCss";
import type { MachineColor } from "./machineIdentity";
import type { ThemeConfig } from "./types";

export const WALLPAPER_PATTERNS = [
  { id: "none", label: "None" },
  { id: "dots", label: "Dots" },
  { id: "domios", label: "Domios", art: new URL("../assets/chat-wallpapers/domios-domios.svg", import.meta.url).href, sample: "aurora" },
  { id: "doodles", label: "Agents", art: new URL("../assets/chat-wallpapers/domios-agents.svg", import.meta.url).href, sample: "ocean" },
  { id: "space", label: "Space", art: new URL("../assets/chat-wallpapers/domios-space.svg", import.meta.url).href, sample: "aurora" },
  { id: "math", label: "Math & code", art: new URL("../assets/chat-wallpapers/domios-math.svg", import.meta.url).href, sample: "ember" },
  { id: "botanical", label: "Botanical", art: new URL("../assets/chat-wallpapers/domios-botanical.svg", import.meta.url).href, sample: "aurora" },
  { id: "retro", label: "Retro games", art: new URL("../assets/chat-wallpapers/domios-retro.svg", import.meta.url).href, sample: "ocean" },
  { id: "geometry", label: "Geometry", art: new URL("../assets/chat-wallpapers/domios-geometry.svg", import.meta.url).href, sample: "sunset" },
  { id: "sketch", label: "Sketchbook", art: new URL("../assets/chat-wallpapers/domios-sketch.svg", import.meta.url).href, sample: "orchid" },
  { id: "circuits", label: "Circuits", art: new URL("../assets/chat-wallpapers/domios-circuits.svg", import.meta.url).href, sample: "ember" },
] as const;

export const PALETTES = {
  aurora: { label: "Aurora", dark: ["#7ee8fa", "#80ff9a", "#b49bff"], light: ["#0a7ea4", "#2b8a3e", "#6d48c8"] },
  sunset: { label: "Sunset", dark: ["#ffb199", "#ff6a88", "#ffd27a"], light: ["#c2410c", "#be185d", "#a16207"] },
  ocean: { label: "Ocean", dark: ["#4facfe", "#00e0ff"], light: ["#1d4ed8", "#0e7490"] },
  orchid: { label: "Orchid", dark: ["#c3a6ff", "#ff9ad5"], light: ["#6d28d9", "#be185d"] },
  ember: { label: "Ember", dark: ["#ffc46b", "#ff5f6d"], light: ["#b45309", "#b91c1c"] },
  mono: { label: "Mono", dark: ["#9aa4ae"], light: ["#5c6670"] },
} satisfies Record<WallpaperPalette, { label: string; light: string[]; dark: string[] }>;

export const FILLS = {
  midnight: { label: "Midnight", dark: ["var(--ct-bg)", "#141b33"], light: ["var(--ct-bg)", "#e9eef8"] },
  dusk: { label: "Dusk", dark: ["#1b1430", "var(--ct-bg)"], light: ["#f4effb", "var(--ct-bg)"] },
  deep_sea: { label: "Deep sea", dark: ["#0a2230", "var(--ct-bg)"], light: ["#eaf5f8", "var(--ct-bg)"] },
  plum: { label: "Plum", dark: ["#26132a", "var(--ct-bg)"], light: ["#fbeff4", "var(--ct-bg)"] },
  forest: { label: "Forest", dark: ["#0f2218", "var(--ct-bg)"], light: ["#ecf6ef", "var(--ct-bg)"] },
} satisfies Record<WallpaperFillPreset, { label: string; light: [string, string]; dark: [string, string] }>;

const hexColor = /^#[0-9a-f]{6}$/i;

export function wallpaperFill(fill: WallpaperFill | undefined, light: boolean): string {
  if (!fill || !hasOwn(FILLS, fill.preset)) return "none";
  const [first, second] = FILLS[fill.preset][light ? "light" : "dark"];
  return fill.mode === "gradient" ? `linear-gradient(160deg, ${first}, ${second})` : first;
}

export function wallpaperPalette(palette: WallpaperPalette | undefined, light: boolean): string {
  if (!palette || !hasOwn(PALETTES, palette)) {
    return "linear-gradient(90deg, #0d1117 50%, #c9d1d9 50%)";
  }
  return inkGradient(PALETTES[palette][light ? "light" : "dark"]);
}

export function wallpaperDefaultOpacity(pattern: ChatPattern, light: boolean, painted: boolean): number {
  if (pattern === "image") return 0.30;
  if (pattern === "dots") return CHAT_BACKGROUND_DEFAULT_OPACITY[light ? "light" : "dark"];
  if (pattern === "svg") return light ? 0.12 : 0.16;
  if (pattern === "grid" || pattern === "icon") return light ? 0.07 : 0.05;
  return painted ? (light ? 0.16 : 0.22) : (light ? 0.09 : 0.085);
}

export function wallpaperDefaultScale(pattern: ChatPattern): number {
  return pattern === "dots" || pattern === "grid" || pattern === "icon" ? 20 : 360;
}

export function effectiveWallpaper(theme: ThemeConfig, setting?: ChatWallpaper): ChatWallpaper {
  return setting ?? { background: theme.chat_background ?? { pattern: "none" } };
}

export function wallpaperRenderStyle(
  theme: ThemeConfig,
  setting: ChatWallpaper | undefined,
  light: boolean,
  assetUrl?: string,
  identity?: MachineColor,
  intensityMultiplier = 1,
): Record<string, string> {
  if (setting?.asset && (!assetUrl || !assetUrl.startsWith("blob:"))) {
    return wallpaperRenderStyle(theme, undefined, light, undefined, identity);
  }
  const wallpaper = effectiveWallpaper(theme, setting);
  const { background, palette, fill } = wallpaper;
  const { pattern } = background;
  const swatch = WALLPAPER_PATTERNS.find(candidate => candidate.id === pattern);
  const art = swatch && "art" in swatch ? swatch.art : undefined;

  if (!setting && (!art || pattern === "doodles") && !background.colors) {
    return backdrop(chatBackgroundToCSS(theme, light));
  }
  const fillStyle = fill ? wallpaperFill(fill, light) : gradientFill(theme, background, light);
  if (pattern === "none") return backdrop(fillStyle);

  const painted = palette !== undefined || Boolean(background.colors?.length || background.color);
  const opacity = Math.min(1, intensityMultiplier * bounded(
    background.opacity, 0, pattern === "image" ? 0.30 : 0.24, wallpaperDefaultOpacity(pattern, light, painted),
  ));
  if (pattern === "image" && assetUrl) {
    const scrim = `color-mix(in srgb, var(--ct-bg) ${Math.round((1 - opacity) * 100)}%, transparent)`;
    return backdrop(`linear-gradient(${scrim}, ${scrim}), url("${assetUrl}") center / cover no-repeat`);
  }
  const scale = bounded(background.scale, 8, 512, wallpaperDefaultScale(pattern));
  let mask: string | undefined;
  if (pattern === "dots") {
    mask = dotsMask(scale);
  } else if (pattern === "grid") {
    const angle = bounded(background.rotation, -360, 360, 0);
    mask = [angle, angle + 90].map(direction => `repeating-linear-gradient(${direction}deg, #fff8 0 1px, transparent 1px ${scale}px)`).join(", ");
  } else {
    const tile = art ?? assetUrl ?? chatPatternTile(background, scale);
    if (tile) mask = `url("${tile}") 0 0 / ${scale}px ${scale}px repeat`;
  }
  return backdrop(fillStyle, mask, inkGradient(inkStops(background, palette, light)), opacity);
}

function gradientFill(theme: ThemeConfig, background: ChatBackground, light: boolean): string {
  return background.gradient
    ? chatBackgroundToCSS({ ...theme, chat_background: { pattern: "none", gradient: background.gradient } }, light)
    : "none";
}

function backdrop(fill: string, mask?: string, ink?: string, opacity?: number): Record<string, string> {
  return {
    "--ct-wallpaper-fill": fill,
    "--ct-wallpaper-mask": mask ?? "none",
    "--ct-wallpaper-ink": mask && ink ? ink : "none",
    "--ct-wallpaper-opacity": String(opacity ?? 0),
  };
}

function inkStops(background: ChatBackground, palette: WallpaperPalette | undefined, light: boolean): string[] {
  if (palette && hasOwn(PALETTES, palette)) return PALETTES[palette][light ? "light" : "dark"];
  const colors = background.colors;
  if (colors && colors.length >= 1 && colors.length <= 3 && colors.every(color => hexColor.test(color))) return colors;
  return [background.color && hexColor.test(background.color) ? background.color : "var(--ct-fg)"];
}

function dotsMask(scale: number): string {
  return `radial-gradient(circle, #fff 0.75px, transparent 1.1px) ${scale / 2}px ${scale / 2}px / ${scale}px ${scale}px repeat`;
}

function hasOwn(record: object, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(record, key);
}

function inkGradient(stops: string[]): string {
  return stops.length === 1 ? stops[0] : `linear-gradient(180deg, ${stops.join(", ")})`;
}

function bounded(value: number | undefined, min: number, max: number, fallback: number): number {
  return value !== undefined && Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : fallback;
}
