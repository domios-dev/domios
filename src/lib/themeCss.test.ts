import { Window } from "happy-dom";
import {
  cacheThemeSnapshot,
  DEFAULT_THEME,
  readCachedTheme,
  readCachedThemeVars,
  themeToCSS,
} from "./themeCss";
import type { ThemeConfig } from "./types";

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
}

{
  cacheThemeSnapshot(lightTheme);
  const cached = readCachedTheme();
  const vars = readCachedThemeVars();
  assert("cached theme restores before backend config", cached?.name === "github-light");
  assert("cached vars restore before React render", vars?.["--ct-bg"] === "#f6f8fa");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
