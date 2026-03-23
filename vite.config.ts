import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { readFileSync } from "fs";

const tauriConf = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf-8"));

export default defineConfig({
  define: {
    __APP_VERSION__: JSON.stringify(tauriConf.version),
  },
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
});
