import { resolve } from "node:path";
import type { Plugin } from "vite";

const scope = resolve(__dirname, "../scope.ts").replace(/\\/g, "/");
const embedDir = resolve(__dirname, "..").replace(/\\/g, "/");

const REWRITES: [RegExp, string][] = [
  [/\bdocument\.(documentElement|body)\b/g, "__film.root()"],
  [/\bdocument\.addEventListener\(/g, "__film.addDocumentListener("],
  [/\bdocument\.removeEventListener\(/g, "__film.removeDocumentListener("],
  [/(["'])(__\w+)\1\s+in\s+window\b/g, "$1$2$1 in __film.globals"],
  [/\bwindow\.(__\w+)/g, "__film.globals.$1"],
  [/\b(?:window|globalThis)\.(fetch|localStorage|sessionStorage|innerWidth|innerHeight|visualViewport|matchMedia)\b/g, "__film.$1"],
  // Calls only: a name at the start of a line or after a method modifier is a method definition (1.22.1's streamMux and sharedRead define `fetch`).
  [/(?<![.\w$'"`])(?<!^[ \t]*)(?<!\b(?:async|private|public|protected|static|override)\s+)(fetch|localStorage|sessionStorage)(?=\s*[.(])/gm, "__film.$1"],
];

// rem inside inline-style strings follows the page's root font size; class names like `p-[0.5rem]` start with `[` and stay.
const INLINE_REM = /(?<=["'`\s(:,])(\d*\.?\d+)rem\b/g;
const TEMPLATE_REM = /\$\{([^{}]+)\}rem\b/g;

/** Rewrites page-global reads and writes in our own source to the film scope; dependencies stay untouched. */
export function scopeGlobals(): Plugin {
  return {
    name: "film-scope-globals",
    enforce: "pre",
    transform(code, id) {
      const file = id.split("?")[0].replace(/\\/g, "/");
      if (!/\.[cm]?[jt]sx?$/.test(file) || file.includes("/node_modules/") || file.startsWith(embedDir)) return null;
      const pixels = code
        .replace(INLINE_REM, (_, n: string) => `${+(Number(n) * 16).toFixed(4)}px`)
        .replace(TEMPLATE_REM, (_, expr: string) => `\${(${expr}) * 16}px`);
      let out = pixels;
      for (const [pattern, replacement] of REWRITES) out = out.replace(pattern, replacement);
      if (out === code) return null;
      return { code: out === pixels ? out : `import { __film } from ${JSON.stringify(scope)};\n${out}`, map: null };
    },
  };
}
