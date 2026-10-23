import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { performance } from "node:perf_hooks";
import { cachedPreparation } from "./preparation-cache.mjs";
import { prepareCompileBundleStubs, requiredPaths } from "./prepare-bundle-resource-stubs.mjs";

const root = process.cwd();
const target = process.argv[2];
if (!/^(x86_64|aarch64)-(apple-darwin|unknown-linux-gnu|pc-windows-msvc)$/.test(target ?? "")) {
  throw new Error(`unsupported compile-stub target: ${target}`);
}
const stage = path.join(root, "target", "domios-package");
const cli = path.join(root, "src-tauri", "bundled-cli", process.platform === "win32" ? "codeterm.exe" : "codeterm");
const start = performance.now();
const reused = cachedPreparation(root, `bundle-${target}`, [
  fileURLToPath(import.meta.url), path.join(root, "scripts", "prepare-bundle-resource-stubs.mjs"),
  path.join(root, "scripts", "preparation-cache.mjs"), cli, stage,
  ...requiredPaths(target).map(file => path.join(stage, file)), path.join(stage, "skills"),
], () => {
  fs.mkdirSync(path.dirname(cli), { recursive: true });
  if (!fs.existsSync(cli)) fs.writeFileSync(cli, "", { flag: "wx" });
  prepareCompileBundleStubs({ root, target });
});
console.log(`[test-loop] bundle-stubs ${reused ? "unchanged" : "prepared"} ${(performance.now() - start).toFixed(1)}ms`);
