import { spawnSync } from "node:child_process";
import { existsSync, writeFileSync, unlinkSync } from "node:fs";
import { fileURLToPath } from "node:url";
const root = fileURLToPath(new URL("../",import.meta.url));
const secrets = new URL("../.secret.local",import.meta.url);
const created = !existsSync(secrets);
if (created) writeFileSync(secrets,[
  "PADDLE_SANDBOX_API_KEY=pdl_sdbx_apikey_fixture",
  "PADDLE_LIVE_API_KEY=pdl_live_apikey_fixture",
  "PADDLE_SANDBOX_WEBHOOK_SECRET=fixture_endpoint_secret",
  "PADDLE_LIVE_WEBHOOK_SECRET=fixture_live_endpoint_secret",
].join("\n")+"\n");
let exit = 1;
try {
  const build = spawnSync(process.execPath,["build.mjs"],{cwd:root,stdio:"inherit"});
  if (build.status === 0) {
    const filters = process.argv.slice(2);
    if (filters.some(value => !/^[a-zA-Z0-9_./-]+$/.test(value))) throw new Error("Invalid test selector");
    const cli = fileURLToPath(new URL("../node_modules/firebase-tools/lib/bin/firebase.js",import.meta.url));
    exit = spawnSync(process.execPath,[cli,"emulators:exec","--non-interactive","--config","../firebase.json","--project","demo-domios","--only","auth,firestore,functions",`vitest run ${filters.join(" ")}`],{cwd:root,stdio:"inherit"}).status ?? 1;
  } else exit = build.status ?? 1;
} finally { if (created) unlinkSync(secrets); }
process.exit(exit);
