import { spawnSync } from "node:child_process";
import { existsSync, writeFileSync, unlinkSync } from "node:fs";
import { fileURLToPath } from "node:url";
const root = fileURLToPath(new URL("../",import.meta.url));
const secrets = new URL("../.secret.local",import.meta.url);
const created = !existsSync(secrets);
if (created) writeFileSync(secrets,[
  "BILLING_TEST_API_KEY=sk_test_fixture",
  "BILLING_LIVE_API_KEY=sk_live_fixture",
  "BILLING_TEST_ENDPOINT_SECRET=whsec_fixture_only",
  "BILLING_LIVE_ENDPOINT_SECRET=whsec_fixture_live_only",
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
