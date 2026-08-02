import { spawnSync } from "node:child_process";

const bash = process.platform === "win32" ? "C:\\Program Files\\Git\\bin\\bash.exe" : "bash";
const result = spawnSync(bash, ["scripts/plugin-tests.sh", ...process.argv.slice(2)], {
  cwd: process.cwd(),
  stdio: "inherit",
});

if (result.error) {
  console.error(`failed to start plugin tests with ${bash}: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
