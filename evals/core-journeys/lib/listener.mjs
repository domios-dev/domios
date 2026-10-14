import { execFileSync } from "node:child_process";
import { join } from "node:path";
import { root } from "./env.mjs";

const probe = "import json, sys; sys.path.insert(0, sys.argv[1]); import listener; print(json.dumps(listener.identity(int(sys.argv[2]))))";

// Reuses restart-endurance's listener identity (PID + kernel start time) without its endurance-only allowlist.
export function listenerIdentity(port) {
  const python = process.env.PYTHON || (process.platform === "win32" ? "python" : "python3");
  return JSON.parse(execFileSync(python, ["-c", probe, join(root, "evals/restart-endurance"), String(port)],
    { encoding: "utf8", timeout: 30_000 }));
}
