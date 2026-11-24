import type { CSSProperties } from "react";
import type { ChatWallpaper } from "../../gen/ChatWallpaper";
import type { AppearanceTuning } from "../../gen/AppearanceTuning";
import type { ThemeConfig } from "./types";
import { blendColor } from "./colors";
import { contrastRatio, DEFAULT_BORDER_STRENGTH, themeToCSS } from "./themeCss";

export type { AppearanceTuning } from "../../gen/AppearanceTuning";

export interface AppearanceLook {
  theme: ThemeConfig;
  wallpaper: ChatWallpaper | null;
  tuning: AppearanceTuning | null;
}

function stableAppearanceKey(value: object): string {
  return JSON.stringify(value, (_key, field: unknown) => {
    if (!field || typeof field !== "object" || Array.isArray(field)) return field;
    const record = field as Record<string, unknown>;
    return Object.fromEntries(Object.keys(record).sort().map(name => [name, record[name]]));
  });
}

export function appearanceLookKey(look: AppearanceLook): string {
  return stableAppearanceKey(look);
}

export function appearancePaletteKey({ chat_background: _wallpaper, name: _name, ...colors }: ThemeConfig): string {
  return stableAppearanceKey(colors);
}

export interface AppearanceMachine { id: string; name: string }
export interface ContrastCheck {
  name: string;
  pair: string;
  ratio: number;
  floor: number;
  fix: { field: string; value: string | number | null } | null;
}
export interface CurrentAppearance {
  machine: AppearanceMachine;
  look: AppearanceLook;
  presets: ThemeConfig[];
  active: AppearanceProposal | null;
}
export interface AppearanceProposal {
  id: string;
  machine: AppearanceMachine;
  rationale: string;
  tweak?: boolean;
  look: AppearanceLook;
  previous: AppearanceLook;
  state: "proposed" | "applied" | "undone";
  applied_at: number | null;
  can_undo: boolean;
  contrast: ContrastCheck[];
}

export const DEFAULT_APPEARANCE: AppearanceTuning = {
  roundness: 1, shadow_strength: 1, border_strength: DEFAULT_BORDER_STRENGTH, font_size: 0,
};

export const PALETTE_CONTROLS = [
  { key: "background", label: "Base" },
  { key: "foreground", label: "Ink" },
  { key: "blue", label: "Accent" },
  { key: "bright_black", label: "Muted" },
  { key: "green", label: "Success" },
  { key: "yellow", label: "Warning" },
  { key: "red", label: "Error" },
] as const;

export const TERMINAL_COLOURS = [
  "cursor", "selection", "black", "magenta", "cyan", "white",
  "bright_red", "bright_green", "bright_yellow", "bright_blue",
  "bright_magenta", "bright_cyan", "bright_white",
] as const;

export function paletteContrast(theme: ThemeConfig, key: typeof PALETTE_CONTROLS[number]["key"]): number | undefined {
  if (key === "background") return undefined;
  const ink = key === "bright_black" ? blendColor(theme.bright_black, theme.foreground, 0.45) : theme[key];
  return contrastRatio(ink, theme.background);
}

export function appearanceTokens(tuning?: AppearanceTuning | null): Record<string, string> {
  const value = tuning ?? DEFAULT_APPEARANCE;
  return {
    "--ct-roundness": String(value.roundness),
    "--ct-shadow-strength": String(value.shadow_strength),
    "--ct-ui-font-offset": `${value.font_size}px`,
  };
}

export function appearancePreviewStyle(look: AppearanceLook): CSSProperties {
  return { ...themeToCSS(look.theme, look.tuning?.border_strength), ...appearanceTokens(look.tuning) } as CSSProperties;
}

export function applyContrastFix(look: AppearanceLook, fix: NonNullable<ContrastCheck["fix"]>): AppearanceLook {
  if (fix.field === "chat_wallpaper.background.opacity" && look.wallpaper && typeof fix.value === "number") {
    return { ...look, wallpaper: { ...look.wallpaper, background: { ...look.wallpaper.background, opacity: fix.value } } };
  }
  if (fix.field === "chat_wallpaper.fill_strength" && look.wallpaper && typeof fix.value === "number") {
    return { ...look, wallpaper: { ...look.wallpaper, fill_strength: fix.value } };
  }
  if (fix.field === "theme.chat_background.opacity" && typeof fix.value === "number") {
    return { ...look, theme: { ...look.theme, chat_background: { ...look.theme.chat_background, pattern: look.theme.chat_background?.pattern ?? "dots", opacity: fix.value } } };
  }
  if (fix.field === "chat_wallpaper.fill" && look.wallpaper) {
    return { ...look, wallpaper: { ...look.wallpaper, fill: undefined } };
  }
  if (fix.field === "chat_wallpaper.background.gradient" && look.wallpaper) {
    return { ...look, wallpaper: { ...look.wallpaper, background: { ...look.wallpaper.background, gradient: undefined } } };
  }
  const palette = PALETTE_CONTROLS.find(({ key }) => fix.field === `theme.${key}`);
  if (!palette || typeof fix.value !== "string") return look;
  return { ...look, theme: { ...look.theme, [palette.key]: fix.value } };
}
