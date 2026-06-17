// Builds a plugin's iframe UI: bundles `plugins/<id>/ui/src/main.tsx` (React/TS)
// into ONE self-contained `plugins/<id>/ui/index.html` with inlined JS+CSS, each
// in a `nonce="__CT_NONCE__"` tag the host's view route swaps for a per-load
// nonce (CSP-locked, VS Code/Figma webview model). The bundle talks to the host
// only through `window.ct` (the bridge shim the host injects).
//
// Usage: node scripts/build-plugin-ui.mjs <pluginId>
import { build } from "esbuild";
import { writeFileSync, mkdirSync, existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const id = process.argv[2];
if (!id) {
  console.error("usage: node scripts/build-plugin-ui.mjs <pluginId>");
  process.exit(1);
}
const entry = resolve(repo, `plugins/${id}/ui/src/main.tsx`);
const outHtml = resolve(repo, `plugins/${id}/ui/index.html`);
if (!existsSync(entry)) {
  console.error(`no UI entry: ${entry}`);
  process.exit(1);
}

const result = await build({
  entryPoints: [entry],
  bundle: true,
  format: "iife",
  jsx: "automatic",
  minify: true,
  write: false,
  // outdir (virtual, write:false) so esbuild emits imported CSS as a sibling
  // output file we inline below, instead of erroring on bare `import "x.css"`.
  outdir: resolve(repo, `plugins/${id}/ui`),
  loader: { ".svg": "text", ".css": "css" },
  define: { "process.env.NODE_ENV": '"production"' },
  logLevel: "warning",
});

let js = "";
let css = "";
for (const f of result.outputFiles) {
  if (f.path.endsWith(".css")) css += f.text;
  else js += f.text;
}

const html = `<!doctype html>
<html>
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<style nonce="__CT_NONCE__">html,body{margin:0}${css}</style>
</head>
<body>
<div id="ct-root"></div>
<script nonce="__CT_NONCE__">${js}</script>
</body>
</html>`;

mkdirSync(dirname(outHtml), { recursive: true });
writeFileSync(outHtml, html);
console.log(`built ${id} ui → plugins/${id}/ui/index.html (${(html.length / 1024).toFixed(1)} KB)`);
