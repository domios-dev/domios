import type { ThemeConfig } from "../../lib/types";

/** Map config cursor_style string to xterm CursorStyle */
export function mapCursorStyle(style?: string): "block" | "underline" | "bar" {
  if (style === "underline") return "underline";
  if (style === "bar") return "bar";
  return "block";
}

/** Build xterm ITheme from ThemeConfig */
export function buildXtermTheme(theme?: ThemeConfig) {
  if (!theme) {
    return {
      background: "#0d1117",
      foreground: "#c9d1d9",
      cursor: "#58a6ff",
      selectionBackground: "#264f78",
      black: "#0d1117",
      red: "#ff7b72",
      green: "#7ee787",
      yellow: "#d29922",
      blue: "#58a6ff",
      magenta: "#bc8cff",
      cyan: "#39d353",
      white: "#c9d1d9",
      brightBlack: "#484f58",
      brightRed: "#ffa198",
      brightGreen: "#56d364",
      brightYellow: "#e3b341",
      brightBlue: "#79c0ff",
      brightMagenta: "#d2a8ff",
      brightCyan: "#56d364",
      brightWhite: "#f0f6fc",
    };
  }
  return {
    background: theme.background,
    foreground: theme.foreground,
    cursor: theme.cursor,
    selectionBackground: theme.selection,
    black: theme.black,
    red: theme.red,
    green: theme.green,
    yellow: theme.yellow,
    blue: theme.blue,
    magenta: theme.magenta,
    cyan: theme.cyan,
    white: theme.white,
    brightBlack: theme.bright_black,
    brightRed: theme.bright_red,
    brightGreen: theme.bright_green,
    brightYellow: theme.bright_yellow,
    brightBlue: theme.bright_blue,
    brightMagenta: theme.bright_magenta,
    brightCyan: theme.bright_cyan,
    brightWhite: theme.bright_white,
    scrollbarSliderBackground: "rgba(128, 128, 128, 0.25)",
    scrollbarSliderHoverBackground: "rgba(128, 128, 128, 0.4)",
    scrollbarSliderActiveBackground: "rgba(128, 128, 128, 0.5)",
    overviewRulerBorder: "#00000000",
  };
}
