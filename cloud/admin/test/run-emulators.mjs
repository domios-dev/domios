import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
const root = fileURLToPath(new URL("../", import.meta.url));
const selectors = process.argv.slice(2);
if (selectors.some(value => !/^[a-zA-Z0-9_./-]+$/.test(value))) throw new Error("Invalid test selector");
const cli = fileURLToPath(new URL("../node_modules/firebase-tools/lib/bin/firebase.js", import.meta.url));
const result = spawnSync(process.execPath, [cli, "emulators:exec", "--non-interactive",
  "--config", "../firebase.admin-test.json", "--project", "demo-domios-admin", "--only", "auth,firestore",
  `vitest run ${selectors.join(" ")}`], { cwd: root, stdio: "inherit", timeout: 180000 });
if (result.error) process.stderr.write(`${result.error.message}\n`);
process.exit(result.status ?? 1);
