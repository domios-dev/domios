import type { ThemeConfig } from "./types";
import { blendColor, deriveColor } from "./colors";

const THEME_CACHE_KEY = "codeterm-theme-v1";
const THEME_VARS_CACHE_KEY = "codeterm-theme-css-v1";

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
  return {
    "--ct-bg": theme.background,
    "--ct-fg": theme.foreground,
    "--ct-cursor": theme.cursor,
    "--ct-selection": theme.selection,
    "--ct-sidebar-bg": deriveColor(theme.background, 8),
    "--ct-titlebar-bg": deriveColor(theme.background, 8),
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
    ...(theme.name.toLowerCase().replace(/[\s-]/g, "") === "pipboy" ? { "--ct-glow": `${theme.green}99` } : {}),
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
