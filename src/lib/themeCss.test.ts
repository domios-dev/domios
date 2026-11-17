import { Window } from "happy-dom";
import {
  cacheThemeSnapshot,
  DEFAULT_THEME,
  readCachedTheme,
  readCachedThemeVars,
  themeToCSS,
} from "./themeCss";
import type { ThemeConfig } from "./types";
import { readFileSync } from "node:fs";
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
