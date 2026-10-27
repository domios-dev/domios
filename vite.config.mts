import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { VitePWA } from "vite-plugin-pwa";
import { readFileSync } from "fs";
import { createRequire } from "module";

const tauriConf = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf-8"));
const require = createRequire(import.meta.url);
const { resolveBuildIdentity, isDevelopmentFrontendBuild } = require("./scripts/build-identity.cjs") as {
  resolveBuildIdentity: () => string;
  isDevelopmentFrontendBuild: (command: string, env: NodeJS.ProcessEnv) => boolean;
};
const htmlTailwindRuntime = readFileSync(require.resolve("@tailwindcss/browser"), "utf-8");

function buildIdentityAsset(buildHash: string): Plugin {
  return {
    name: "codeterm-build-identity",
    generateBundle() {
      this.emitFile({
        type: "asset",
        fileName: "build-identity.json",
        source: `${JSON.stringify({ buildHash })}\n`,
      });
    },
  };
}

export default defineConfig(({ command }) => {
  const buildHash = resolveBuildIdentity();
  return {
    // AND from the daemon's /web mount (web mirror) without path rewrites.
    base: "./",
    define: {
      __APP_VERSION__: JSON.stringify(tauriConf.version),
      __BUILD_HASH__: JSON.stringify(buildHash),
      __CODETERM_DEV__: JSON.stringify(isDevelopmentFrontendBuild(command, process.env)),
      __CODETERM_HTML_TAILWIND_RUNTIME__: JSON.stringify(htmlTailwindRuntime),
    },
    plugins: [
      // React's dev build diffs every changed prop object into the Performance track when console.timeStamp exists;
      // with hundreds of tab rows that froze typing for seconds, so the dev server hides it before React loads.
      { name: "dev-no-react-perf-track", apply: "serve", transformIndexHtml: () => [{ tag: "script", children: "console.timeStamp = undefined;", injectTo: "head-prepend" }] },
      react(),
      tailwindcss(),
      buildIdentityAsset(buildHash),
      // injectManifest: explicit SW keeps /api/* network-only passthrough auditable.
      VitePWA({
        strategies: "injectManifest",
        srcDir: "src",
        filename: "sw.ts",
        injectRegister: false,
        manifest: false,
        injectManifest: {
          globPatterns: ["**/*.{js,css,html,ico,png,svg,woff2}"],
          globIgnores: ["**/sw.ts", "**/sw.js", "**/sw.mjs"],
          maximumFileSizeToCacheInBytes: 8 * 1024 * 1024,
          rollupFormat: "iife",
        },
        devOptions: { enabled: false },
      }),
    ],
    clearScreen: false,
    // The repo vendors unrelated HTML under references/ and refs/; scan only the app entry.
    optimizeDeps: { entries: ["index.html"] },
    server: {
      host: "127.0.0.1",
      // Per-worktree dev isolation: build.sh derives a stable per-instance port.
      port: Number(process.env.CODETERM_VITE_PORT) || 1420,
      strictPort: true,
      // `build.sh dev-front` serves the native DEV window; daemon routes stay same-origin like the static server.
      // Whole path segments only: a bare "/frontend" prefix would also swallow /frontend-release-contract.json.
      proxy: {
        "^/(api|frontend|bundled)(/|$)": {
          target: `http://127.0.0.1:${process.env.CODETERM_API_PORT || 7686}`,
          ws: true,
        },
      },
      watch: {
        ignored: ["**/src-tauri/target/**", "**/cli/target/**", "**/target-check/**", "**/target/**", "**/src-tauri/crates/**", "**/references/**", "**/refs/**"],
      },
    },
    envPrefix: ["VITE_", "TAURI_"],
  };
});
