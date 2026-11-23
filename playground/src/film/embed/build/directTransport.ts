import { resolve } from "node:path";
import type { Plugin } from "vite";

const transport = resolve(__dirname, "../directTransport.ts").replace(/\\/g, "/");
const galleryTransport = resolve(__dirname, "../../../mocks/daemonTransport.ts").replace(/\\/g, "/");

/**
 * Since 1.22 the app multiplexes every daemon request over one carrier stream, and the gallery
 * config (which the film config extends) swaps that mux for the gallery's mock fetch. The film has
 * its own fixture routes that answer plain fetches, so the gallery's transport module loads the
 * film's plain transport instead. daemonBootstrap itself is untouched.
 */
export function directDaemonTransport(): Plugin {
  return {
    name: "film-direct-daemon-transport",
    enforce: "pre",
    load(id) {
      if (id.split("?")[0].replace(/\\/g, "/") === galleryTransport) return `export { withStreamMux } from ${JSON.stringify(transport)};\n`;
      return null;
    },
  };
}
