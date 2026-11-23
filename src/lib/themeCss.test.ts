import { Window } from "happy-dom";
import {
  cacheThemeSnapshot,
  DEFAULT_THEME,
  isLightColor,
  readCachedTheme,
  readCachedThemeVars,
  themeToCSS,
} from "./themeCss";
import type { ThemeConfig } from "./types";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { blendColor, parseHex } from "./colors";

let passed = 0;
let failed = 0;

function assert(label: string, ok: boolean, detail?: string) {
  if (ok) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}${detail ? ` — ${detail}` : ""}`);
    failed++;
  }
}

function setupDom() {
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "localStorage", { value: window.localStorage, configurable: true });
}

setupDom();

const lightTheme: ThemeConfig = {
  ...DEFAULT_THEME,
  name: "github-light",
  background: "#f6f8fa",
  foreground: "#1f2328",
  blue: "#0969da",
  bright_black: "#57606a",
};

console.log("themeCss tests");

for (const [theme, opacity] of [[lightTheme, 7], [DEFAULT_THEME, 5]] as const) {
  const backdrop = themeToCSS(theme)["--ct-chat-backdrop"];
  assert(`${theme.name}: default backdrop tiles one foreground-colored dot`,
    backdrop === `radial-gradient(circle, color-mix(in srgb, var(--ct-fg) ${opacity}%, transparent) 0.75px, transparent 1.1px) 10px 10px / 20px 20px repeat`);
}

{
  const theme = {
    ...lightTheme,
    chat_background: {
      pattern: "icon",
      icon: "cat",
      scale: 48,
      rotation: -12,
      color: "#5359a8",
      opacity: 1,
      gradient: { kind: "linear", angle: 35, stops: ["#5359a8", "#419a80"] },
    },
  } satisfies ThemeConfig;
  const backdrop = themeToCSS(theme)["--ct-chat-backdrop"];
  const encodedTile = backdrop.match(/^url\("data:image\/svg\+xml,([^"]+)"\)/)?.[1] ?? "";
  const decoded = decodeURIComponent(encodedTile);
  assert("custom tile repeats at 48 px", backdrop.includes("/ 48px 48px repeat"));
  assert("cat glyph respects the rotation, color and readability cap",
    decoded.includes('<path d="M5 10')
      && decoded.includes('rotate(-12 24 24)')
      && decoded.includes('color="#5359a8"')
      && decoded.includes('opacity="0.12"'));
  assert("a subdued gradient paints below the tile", backdrop.startsWith('url("data:image/svg+xml,')
    && backdrop.endsWith("linear-gradient(35deg, color-mix(in srgb, #5359a8 8%, transparent), color-mix(in srgb, #419a80 8%, transparent)) no-repeat"));

  const none = themeToCSS({ ...theme, chat_background: { pattern: "none" } });
  assert("none clears the previous background", none["--ct-chat-backdrop"] === "none");
  const radial = themeToCSS({ ...theme, chat_background: {
    pattern: "none",
    gradient: { ...theme.chat_background.gradient, kind: "radial" },
  } });
  assert("gradient alone fills the transcript", radial["--ct-chat-backdrop"].startsWith("radial-gradient("));

  const grid = themeToCSS({ ...theme, chat_background: { pattern: "grid", scale: 32, rotation: 20 } });
  assert("grid repeats rotated perpendicular lines at the requested spacing",
    grid["--ct-chat-backdrop"].includes("repeating-linear-gradient(20deg,")
      && grid["--ct-chat-backdrop"].includes("repeating-linear-gradient(110deg,")
      && grid["--ct-chat-backdrop"].includes("transparent 1px 32px"));
}

for (const [theme, opacity] of [[lightTheme, 0.09], [DEFAULT_THEME, 0.085]] as const) {
  const painted: ThemeConfig = { ...theme, chat_background: { pattern: "doodles" } };
  const vars = themeToCSS(painted);
  const backdrop = vars["--ct-chat-backdrop"];
  assert(`${theme.name}: doodles use the theme ink and a 360 px tile`,
    backdrop.includes("/ 360px 360px repeat")
      && backdrop.includes(`color%3D%22%23${theme.foreground.slice(1)}%22`)
      && backdrop.includes(`opacity%3D%22${opacity}%22`));
  cacheThemeSnapshot(painted, vars);
  assert("first-paint cache excludes the doodle SVG and preserves theme colors",
    readCachedTheme()?.chat_background === undefined
      && readCachedThemeVars()?.["--ct-chat-backdrop"] === undefined
      && readCachedThemeVars()?.["--ct-bg"] === theme.background);
}

{
  const vars = themeToCSS(lightTheme);
  assert("themeToCSS uses theme background", vars["--ct-bg"] === "#f6f8fa");
  assert("themeToCSS uses theme foreground", vars["--ct-fg"] === "#1f2328");
  assert("themeToCSS derives elevated background", vars["--ct-bg-elev"] !== "#0d1117");
  assert("dark blue accent uses white text", vars["--ct-on-accent"] === "#ffffff");
}

{
  const vars = themeToCSS(DEFAULT_THEME);
  assert("light blue accent uses black text", vars["--ct-on-accent"] === "#000000");
  assert("dark mentions expose an accessible text color", vars["--ct-mention-text"].startsWith("#") || vars["--ct-mention-text"] === "var(--ct-accent)");
}

{
  const vars = themeToCSS({ ...lightTheme, blue: "#58a6ff" });
  assert("low contrast light mentions darken toward foreground", vars["--ct-mention-text"].startsWith("#"));
}

{
  cacheThemeSnapshot(lightTheme);
  const cached = readCachedTheme();
  const vars = readCachedThemeVars();
  assert("cached theme restores before backend config", cached?.name === "github-light");
  assert("cached vars restore before React render", vars?.["--ct-bg"] === "#f6f8fa");
}

{
  const html = readFileSync("index.html", "utf8");
  const bootstrap = html.match(/<script>([\s\S]*?)<\/script>/)?.[1] ?? "";
  for (const background of [lightTheme.background, DEFAULT_THEME.background, "#50ce50", "#c080c0"]) {
    setupDom();
    cacheThemeSnapshot({ ...lightTheme, background });
    runInNewContext(bootstrap, { document: window.document, localStorage: window.localStorage });
    const root = window.document.documentElement;
    const expected = isLightColor(background) ? "light" : "dark";
    assert(`first paint restores the app theme mode for ${background}`, root.dataset.ctTheme === expected && root.style.colorScheme === expected);
    assert(`first paint restores the background variable for ${background}`, root.style.getPropertyValue("--ct-bg") === background);
  }
}

for (const name of ["github-dark", "github-light", "dracula"]) {
  const values = Object.fromEntries([...readFileSync(`src-tauri/themes/${name}.toml`, "utf8").matchAll(/^(\w+) = "([^"]+)"/gm)].map(match => [match[1], match[2]])) as unknown as ThemeConfig;
  const vars = themeToCSS(values);
  const own = blendColor(vars["--ct-surface"], values.blue, .08);
  const relayed = blendColor(values.background, values.foreground, .04);
  const luminance = (hex: string) => parseHex(hex).map(channel => {
    const value = channel / 255;
    return value <= .04045 ? value / 12.92 : ((value + .055) / 1.055) ** 2.4;
  }).reduce((sum, value, index) => sum + value * [.2126, .7152, .0722][index], 0);
  const contrast = (a: string, b: string) => (Math.max(luminance(a), luminance(b)) + .05) / (Math.min(luminance(a), luminance(b)) + .05);
  const text = vars["--ct-mention-text"] === "var(--ct-accent)" ? values.blue : vars["--ct-mention-text"];
  for (const bubble of [own, relayed]) {
    assert(`${name}: bubble foreground contrast >= 7`, contrast(values.foreground, bubble) >= 7);
    assert(`${name}: mention contrast >= 4.5`, contrast(text, blendColor(bubble, values.blue, .12)) >= 4.5);
    assert(`${name}: hovered mention contrast >= 4.5`, contrast(text, blendColor(bubble, values.blue, .18)) >= 4.5);
    assert(`${name}: muted mention contrast >= 4.5`, contrast(blendColor(vars["--ct-surface"], values.foreground, .75), bubble) >= 4.5);
  }
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
