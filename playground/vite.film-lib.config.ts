// Embeddable live film: one ESM entry plus one stylesheet the module loads into its own shadow root.
// Build: npx vite build --config playground/vite.film-lib.config.ts
// Serve the embed test page under the site's CSP: npx vite preview --config playground/vite.film-lib.config.ts --outDir .
import { copyFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { mergeConfig, type Plugin } from "vite";
import base from "./vite.config";
import { fileAssets } from "./src/film/embed/build/fileAssets";
import { scopeGlobals } from "./src/film/embed/build/scopeGlobals";
import { directDaemonTransport } from "./src/film/embed/build/directTransport";
import { shadowCss } from "./src/film/embed/build/shadowCss";

const outDir = resolve(__dirname, "dist-film");
const media = resolve(__dirname, "public/film-audio");
const CLIPS = ["landscape", "portrait"].flatMap((layout) => ["dark", "light"].map((theme) => `web-hale.fallback-${layout}-${theme}.mp4`));
const PUBLIC_FILES = ["web-hale.m4a", "web-hale.captions.json", ...CLIPS];
// The full Prism grammar pack loads only for languages the film never shows; its regex sources also read as eval( to CSP audits.
const UNUSED = /^react-syntax-highlighter\/dist\/esm\/prism$/;

const CSP = [
  "default-src 'self'",
  "script-src 'self'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self'",
  "media-src 'self'",
  "connect-src 'self'",
  "font-src 'self'",
].join("; ");

function sitePublic(): Plugin {
  return {
    name: "film-site-public",
    apply: "build",
    closeBundle() {
      const missing = PUBLIC_FILES.filter((f) => !existsSync(resolve(media, f)));
      if (missing.length) throw new Error(`missing film media in ${media}: ${missing.join(", ")}; fallback clips come from playground/src/film/embed/build/fallback-clips.mjs`);
      mkdirSync(resolve(outDir, "public"), { recursive: true });
      for (const f of PUBLIC_FILES) copyFileSync(resolve(media, f), resolve(outDir, "public", f));
      writeFileSync(resolve(outDir, ".gitignore"), "*\n");
    },
  };
}

export default mergeConfig(base, {
  plugins: [directDaemonTransport(), scopeGlobals(), fileAssets(), shadowCss("film.css"), sitePublic()],
  publicDir: false,
  resolve: {
    alias: [
      { find: /^lucide-react\/dynamic$/, replacement: resolve(__dirname, "src/film/embed/build/staticIcon.tsx") },
      { find: UNUSED, replacement: resolve(__dirname, "src/film/embed/build/emptyModule.ts") },
    ],
  },
  // A public build: no development-build dot in the titlebar, no development diagnostics.
  define: { "process.env.NODE_ENV": JSON.stringify("production"), __CODETERM_DEV__: "false" },
  build: {
    outDir,
    emptyOutDir: true,
    cssCodeSplit: false,
    sourcemap: false,
    // Library builds keep ES output readable by default; the site serves it as is, so it ships minified.
    minify: true,
    lib: { entry: resolve(__dirname, "src/film/embed/index.ts"), formats: ["es"], fileName: () => "index.js" },
    rolldownOptions: {
      // Library mode leaves whitespace in ES output; the bundler's own minifier takes it out.
      output: { chunkFileNames: "chunks/[name]-[hash].js", assetFileNames: "assets/[name]-[hash][extname]", minify: true },
    },
  },
  preview: { host: "127.0.0.1", port: 5430, strictPort: true, headers: { "Content-Security-Policy": CSP } },
});
