import { spawn } from "node:child_process";
import { join } from "node:path";
import { bashInterpreter } from "../../../scripts/bash-interpreter.mjs";
import { assertIsolated, client, root, target } from "./env.mjs";

export async function neighborPeer({ instance, port }) {
  assertIsolated({ instance, port });
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("J5 peer port is invalid");
  const output = await new Promise((accept, reject) => {
    const child = spawn(bashInterpreter(), [join(root, "evals/core-journeys/run.sh"), "--connection"], {
      cwd: root, stdio: ["ignore", "pipe", "pipe"], signal: AbortSignal.timeout(60_000),
      env: { ...process.env, CODETERM_INSTANCE: instance, CODETERM_API_PORT: String(port) },
    });
    let stdout = "";
    child.stdout.on("data", chunk => { stdout += chunk; });
    child.stderr.resume();
    child.on("error", () => reject(new Error("J5 peer connection failed")));
    child.on("close", code => code === 0 ? accept(stdout) : reject(new Error(`J5 peer connection exit ${code}`)));
  });
  const dev = target(JSON.parse(output));
  if (dev.instance !== instance || dev.port !== port) throw new Error("J5 peer connection identity mismatch");
  return { dev, api: client(dev).api };
}

if (process.argv[2] === "--emit-connection") {
  target();
  const fields = ["INSTANCE", "API_PORT", "API_URL", "API_TOKEN", "DATA_DIR"];
  process.stdout.write(JSON.stringify(Object.fromEntries(fields.map(field => [`CORE_JOURNEY_${field}`, process.env[`CORE_JOURNEY_${field}`]]))));
}
