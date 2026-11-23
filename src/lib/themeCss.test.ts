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

{
  setupDom();
  const { document } = window;
  const root = document.documentElement;
  root.dataset.ctTheme = isLightColor(DEFAULT_THEME.background) ? "light" : "dark";
  const tokens = document.createElement("style");
  tokens.textContent = readFileSync("src/styles/tokens.css", "utf8");
  document.head.append(tokens);

  const expectedDarkShadows = {
    "--ct-shadow-sm": "0 1px 2px rgb(0 0 0/.20), 0 1px 3px rgb(0 0 0/.10)",
    "--ct-shadow-md": "0 1px 3px rgb(0 0 0/.20), 0 3px 10px rgb(0 0 0/.13), 0 0 0 1px rgb(255 255 255/.04)",
    "--ct-shadow-lg": "0 3px 6px rgb(0 0 0/.20), 0 8px 24px rgb(0 0 0/.16), 0 0 0 1px rgb(255 255 255/.06)",
    "--ct-shadow-xl": "0 6px 12px rgb(0 0 0/.23), 0 16px 44px rgb(0 0 0/.20), 0 0 0 1px rgb(255 255 255/.06)",
    "--ct-shadow-capsule": "0 2px 6px rgb(0 0 0/.16), 0 8px 22px rgb(0 0 0/.13), 0 0 0 1px rgb(255 255 255/.06)",
  };
  const computed = window.getComputedStyle(root);
  for (const [token, expected] of Object.entries(expectedDarkShadows)) {
    const actual = computed.getPropertyValue(token).replace(/\s+/g, " ").trim();
    assert(`github-dark resolves ${token}`, actual === expected, `expected ${expected}, got ${actual}`);
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
