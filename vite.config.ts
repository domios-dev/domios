import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { readFileSync } from "fs";

const tauriConf = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf-8"));

export default defineConfig({
  // Relative asset URLs: the same bundle loads from the Tauri webview root
  // AND from the daemon's /web mount (web mirror) without path rewrites.
  base: "./",
  define: {
    __APP_VERSION__: JSON.stringify(tauriConf.version),
  },
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
});
