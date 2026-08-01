import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import net from "node:net";

const [mode, first, second] = process.argv.slice(2);
const marker = mode === "cargo" ? second : first;

function endpoint(suffix) {
  return process.platform === "win32"
    ? `\\\\.\\pipe\\${marker}-${suffix}`
    : `/tmp/${marker}-${suffix}.sock`;
}

if (mode === "rustc") {
  net.createServer().listen(endpoint("rustc"), () => console.log("ready"));
} else if (mode === "cargo") {
  const rustc = spawn(
    first,
    [fileURLToPath(import.meta.url), "rustc", marker],
    { stdio: ["ignore", "pipe", "ignore"] },
  );
  rustc.stdout.once("data", () => {
    net.createServer().listen(endpoint("cargo"), () => console.log("ready"));
  });
} else {
  process.exit(2);
}
