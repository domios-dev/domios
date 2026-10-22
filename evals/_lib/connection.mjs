import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function devConnection() {
  const gitBash = "C:/Program Files/Git/bin/bash.exe";
  const bash = process.platform === "win32" && existsSync(gitBash) ? gitBash : "bash";
  const lib = fileURLToPath(new URL("./lib.sh", import.meta.url));
  const env = { ...process.env };
  if (env.CODETERM_DEV_INSTANCE) env.CODETERM_INSTANCE = env.CODETERM_DEV_INSTANCE;
  if (env.CODETERM_DEV_API_PORT) env.CODETERM_API_PORT = env.CODETERM_DEV_API_PORT;
  const output = execFileSync(bash, ["-c",
    `source "$1"; dev_run python3 -c 'import json,os; print(json.dumps({"api":os.environ["CODETERM_API_URL"],"token":os.environ["CODETERM_API_TOKEN"],"dataDir":os.environ["CODETERM_DATA_DIR"]}))'`,
    "dev-connection", resolve(lib)], { env, encoding: "utf8", windowsHide: true });
  return JSON.parse(output);
}
