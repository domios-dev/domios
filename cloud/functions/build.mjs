import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
for (const config of ["tsconfig.json", "tsconfig.web.json"]) {
  const result = spawnSync(process.execPath, [fileURLToPath(new URL("node_modules/typescript/bin/tsc", import.meta.url)), "--project", config], {stdio: "inherit"});
  if (result.status !== 0) process.exit(result.status ?? 1);
}
await build({
  entryPoints: ["../hosting/signin/main.ts"], bundle: true, format: "esm", target: "es2022",
  outfile: "../hosting/signin/app.js", nodePaths: [fileURLToPath(new URL("node_modules", import.meta.url))],
});
