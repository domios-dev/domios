import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { VitePWA } from "vite-plugin-pwa";
import { readFileSync } from "fs";
import { createRequire } from "module";
import { isDevelopmentFrontendBuild } from "./src/components/Titlebar/buildIdentity";

const tauriConf = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf-8"));
const require = createRequire(import.meta.url);
const { resolveBuildIdentity } = require("./scripts/build-identity.cjs") as {
  resolveBuildIdentity: () => string;
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
          maximumFileSizeToCacheInBytes: 4 * 1024 * 1024,
          rollupFormat: "iife",
        },
        devOptions: { enabled: false },
      }),
    ],
    clearScreen: false,
    server: {
      host: "127.0.0.1",
      // Per-worktree dev isolation: build.sh derives a stable per-instance port.
      port: Number(process.env.CODETERM_VITE_PORT) || 1420,
      strictPort: true,
    },
    envPrefix: ["VITE_", "TAURI_"],
  };
});
