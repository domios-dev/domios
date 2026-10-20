// Caller: build.sh chat-scroll-trace; skill dev-testing
// Serves the chat scroll harness on a free local port and prints one "ready <url>" line once it listens.
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { createServer } from "vite";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "../..");
const server = await createServer({
  configFile: false,
  root: here,
  plugins: [react()],
  define: { __CODETERM_DEV__: "false" },
  server: { host: "127.0.0.1", port: 0, fs: { allow: [repo] } },
  cacheDir: path.join(os.tmpdir(), "codeterm-chat-scroll-trace-vite"),
  logLevel: "error",
});
await server.listen();
const { port } = server.httpServer.address();
console.log(`ready http://127.0.0.1:${port}/`);
