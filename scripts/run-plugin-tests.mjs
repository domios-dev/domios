import { spawnSync } from "node:child_process";
import { delimiter, dirname } from "node:path";

const bash = process.platform === "win32" ? "C:\\Program Files\\Git\\bin\\bash.exe" : "bash";
const result = spawnSync(bash, ["scripts/plugin-tests.sh", ...process.argv.slice(2)], {
  cwd: process.cwd(),
  env: {
    ...process.env,
    PATH: `${dirname(process.execPath)}${delimiter}${process.env.PATH || ""}`,
    CODETERM_NODE_BIN: process.execPath.replaceAll("\\", "/"),
  },
  stdio: "inherit",
});

if (result.error) {
  console.error(`failed to start plugin tests with ${bash}: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
