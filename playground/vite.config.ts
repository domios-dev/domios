import { resolve } from "node:path";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig, type Plugin } from "vite";

const productSrc = resolve(__dirname, "../src").replace(/\\/g, "/");
const mocks = resolve(__dirname, "src/mocks");

const TAURI_MOCKS: Record<string, string> = {
  "@tauri-apps/api/core": "tauri-core.ts",
  "@tauri-apps/api/event": "tauri-event.ts",
  "@tauri-apps/api/window": "tauri-window.ts",
  "@tauri-apps/api/webview": "tauri-webview.ts",
  "@tauri-apps/plugin-dialog": "tauri-dialog.ts",
};

// Product portals land in the gallery stage that rendered them instead of document.body.
function stagePortals(): Plugin {
  return {
    name: "gallery-stage-portals",
    enforce: "pre",
    resolveId(source, importer) {
      if (source !== "react-dom" || !importer) return null;
      return importer.replace(/\\/g, "/").startsWith(productSrc) ? resolve(mocks, "react-dom.tsx") : null;
    },
  };
}

export default defineConfig({
  root: __dirname,
  plugins: [stagePortals(), react(), tailwindcss()],
  resolve: {
    alias: Object.entries(TAURI_MOCKS).map(([find, file]) => ({ find: new RegExp(`^${find}$`), replacement: resolve(mocks, file) })),
  },
  define: {
    __APP_VERSION__: JSON.stringify("gallery"),
    __BUILD_HASH__: JSON.stringify("gallery"),
    __CODETERM_DEV__: "true",
    __CODETERM_HTML_TAILWIND_RUNTIME__: JSON.stringify(""),
  },
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1431,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
