import { DEFAULT_THEME, isLightColor, themeToCSS } from "../../../../src/lib/themeCss";

export type FilmTheme = "light" | "dark";

const LIGHT = { ...DEFAULT_THEME, name: "github-light", background: "#f6f8fa", foreground: "#1f2328", cursor: "#0969da", selection: "#ace2f9", black: "#24292f", red: "#cf222e", green: "#1a7f37", yellow: "#9a6700", blue: "#0969da", magenta: "#8250df", cyan: "#1b7c83", white: "#6e7781", bright_black: "#57606a", bright_red: "#ff8182", bright_green: "#4ac26b", bright_yellow: "#d4a72c", bright_blue: "#54aeff", bright_magenta: "#c297ff", bright_cyan: "#3db8c1", bright_white: "#8c959f" };

/** Writes the product palette onto `root`; canvas scenes read it back from there on their next frame. */
export function applyFilmTheme(root: HTMLElement, theme: FilmTheme) {
  const palette = theme === "light" ? LIGHT : DEFAULT_THEME;
  root.dataset.ctTheme = isLightColor(palette.background) ? "light" : "dark";
  root.style.colorScheme = root.dataset.ctTheme;
  for (const [name, value] of Object.entries(themeToCSS(palette))) root.style.setProperty(name, value);
}
