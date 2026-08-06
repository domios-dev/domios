import type { ThemeConfig } from "./types";
import { blendColor, deriveColor, parseHex } from "./colors";

const THEME_CACHE_KEY = "codeterm-theme-v1";
const THEME_VARS_CACHE_KEY = "codeterm-theme-css-v1";

function contrastTextColor(background: string): "#000000" | "#ffffff" {
  const [r, g, b] = parseHex(background).map((channel) => {
    const value = channel / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  const luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  const blackContrast = (luminance + 0.05) / 0.05;
  const whiteContrast = 1.05 / (luminance + 0.05);
  return blackContrast >= whiteContrast ? "#000000" : "#ffffff";
}

export const DEFAULT_THEME: ThemeConfig = {
  name: "github-dark",
  background: "#0d1117",
  foreground: "#c9d1d9",
  cursor: "#58a6ff",
  selection: "#264f78",
  black: "#0d1117",
  red: "#ff7b72",
  green: "#7ee787",
  yellow: "#d29922",
  blue: "#58a6ff",
  magenta: "#bc8cff",
  cyan: "#39d353",
  white: "#c9d1d9",
  bright_black: "#484f58",
  bright_red: "#ffa198",
  bright_green: "#56d364",
  bright_yellow: "#e3b341",
  bright_blue: "#79c0ff",
  bright_magenta: "#d2a8ff",
  bright_cyan: "#56d364",
  bright_white: "#f0f6fc",
};

export function themeToCSS(theme: ThemeConfig): Record<string, string> {
  const isLight = isLightColor(theme.background);
  const syntax = syntaxColors(theme, isLight);
  const chromeBg = deriveColor(theme.background, 8);
  return {
    "--ct-bg": theme.background,
    "--ct-fg": theme.foreground,
    "--ct-cursor": theme.cursor,
    "--ct-selection": theme.selection,
    "--ct-sidebar-bg": chromeBg,
    "--ct-titlebar-bg": chromeBg,
    "--ct-tray-divider": isLight
      ? blendColor(chromeBg, theme.bright_black, 0.34)
      : blendColor(chromeBg, theme.bright_white, 0.58),
    "--ct-on-accent": contrastTextColor(theme.blue),
    "--ct-border": blendColor(theme.background, theme.bright_black, 0.38),
    "--ct-hover": deriveColor(theme.background, 14),
    "--ct-active": deriveColor(theme.background, 22),
    "--ct-surface": deriveColor(theme.background, 10),
    "--ct-bg-elev": deriveColor(theme.background, 10),
    "--ct-bg-hover": deriveColor(theme.background, 14),
    "--ct-bg-active": deriveColor(theme.background, 22),
    "--ct-bg-selected": blendColor(theme.background, theme.blue, 0.22),
    "--ct-border-subtle": blendColor(theme.background, theme.bright_black, 0.2),
    "--ct-border-default": blendColor(theme.background, theme.bright_black, 0.38),
    "--ct-muted": theme.bright_black,
    "--ct-muted-fg": blendColor(theme.bright_black, theme.foreground, 0.45),
    "--ct-accent": theme.blue,
    "--ct-red": theme.red,
    "--ct-green": theme.green,
    "--ct-yellow": theme.yellow,
    "--ct-orange": theme.red,
    "--ct-blue": theme.blue,
    "--ct-magenta": theme.magenta,
    "--ct-cyan": theme.cyan,
    "--ct-bright-blue": theme.bright_blue,
    "--ct-bright-magenta": theme.bright_magenta,
    "--ct-bright-cyan": theme.bright_cyan,
    "--ct-bright-green": theme.bright_green,
    "--ct-bright-yellow": theme.bright_yellow,
    "--ct-bright-red": theme.bright_red,
    "--ct-diff-del-bg": blendColor(theme.background, theme.red, isLight ? 0.08 : 0.18),
    "--ct-diff-add-bg": blendColor(theme.background, theme.green, isLight ? 0.08 : 0.18),
    "--ct-diff-del-fg": syntax.deleted,
    "--ct-diff-add-fg": syntax.inserted,
    "--ct-syntax-fg": syntax.fg,
    "--ct-syntax-muted": syntax.muted,
    "--ct-syntax-comment": syntax.comment,
    "--ct-syntax-punctuation": syntax.punctuation,
    "--ct-syntax-keyword": syntax.keyword,
    "--ct-syntax-string": syntax.string,
    "--ct-syntax-number": syntax.number,
    "--ct-syntax-function": syntax.function,
    "--ct-syntax-type": syntax.type,
    "--ct-syntax-property": syntax.property,
    "--ct-syntax-operator": syntax.operator,
    "--ct-syntax-tag": syntax.tag,
    "--ct-syntax-deleted": syntax.deleted,
    "--ct-syntax-inserted": syntax.inserted,
    ...(theme.name.toLowerCase().replace(/[\s-]/g, "") === "pipboy" ? { "--ct-glow": `${theme.green}99` } : {}),
  };
}

export function isLightColor(hex: string): boolean {
  const [r, g, b] = parseHex(hex);
  return (r * 299 + g * 587 + b * 114) / 1000 > 150;
}

function readableAccent(color: string, isLight: boolean): string {
  return blendColor(color, isLight ? "#000000" : "#ffffff", isLight ? 0.28 : 0.12);
}

function syntaxColors(theme: ThemeConfig, isLight: boolean) {
  return {
    fg: theme.foreground,
    muted: blendColor(theme.foreground, theme.background, isLight ? 0.36 : 0.28),
    comment: blendColor(theme.bright_black, theme.foreground, isLight ? 0.44 : 0.34),
    punctuation: blendColor(theme.foreground, theme.background, isLight ? 0.22 : 0.18),
    keyword: readableAccent(theme.blue, isLight),
    string: readableAccent(theme.green, isLight),
    number: readableAccent(theme.yellow, isLight),
    function: readableAccent(theme.magenta, isLight),
    type: readableAccent(theme.cyan, isLight),
    property: readableAccent(theme.bright_blue, isLight),
    operator: readableAccent(theme.foreground, isLight),
    tag: readableAccent(theme.red, isLight),
    deleted: readableAccent(theme.red, isLight),
    inserted: readableAccent(theme.green, isLight),
  };
}

export function cacheThemeSnapshot(theme: ThemeConfig) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(THEME_CACHE_KEY, JSON.stringify(theme));
    window.localStorage.setItem(THEME_VARS_CACHE_KEY, JSON.stringify(themeToCSS(theme)));
  } catch {
    /* best-effort first-paint cache */
  }
}

export function readCachedTheme(): ThemeConfig | null {
  if (typeof window === "undefined") return null;
  try {
    const parsed = JSON.parse(window.localStorage.getItem(THEME_CACHE_KEY) ?? "null");
    return isThemeConfig(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

export function readCachedThemeVars(): Record<string, string> | null {
  if (typeof window === "undefined") return null;
  try {
    const parsed = JSON.parse(window.localStorage.getItem(THEME_VARS_CACHE_KEY) ?? "null");
    if (!parsed || typeof parsed !== "object") return null;
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(parsed)) {
      if (k.startsWith("--ct-") && typeof v === "string") out[k] = v;
    }
    return Object.keys(out).length > 0 ? out : null;
  } catch {
    return null;
  }
}

function isThemeConfig(value: unknown): value is ThemeConfig {
  if (!value || typeof value !== "object") return false;
  const theme = value as Record<string, unknown>;
  return (
    typeof theme.name === "string" &&
    typeof theme.background === "string" &&
    typeof theme.foreground === "string" &&
    typeof theme.blue === "string" &&
    typeof theme.bright_black === "string"
  );
}
