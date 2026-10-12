import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync, rmSync } from "node:fs";
import { test } from "node:test";
import { signinConfig } from "./write-signin-config.mjs";

test("writer persists the injected Firebase configuration and fails clearly without it", () => {
  const env = {...process.env, DOMIOS_FIREBASE_API_KEY:"test-key", DOMIOS_FIREBASE_PROJECT_ID:"test-project", DOMIOS_FIREBASE_APP_ID:"test-app", DOMIOS_FIREBASE_APP_CHECK_SITE_KEY:"test-site-key"};
  const target = new URL("./hosting/signin/config.json", import.meta.url);
  try {
    const result = spawnSync(process.execPath, [new URL("./write-signin-config.mjs", import.meta.url).pathname], {env, encoding:"utf8"});
    assert.equal(result.status, 0, result.stderr);
    const saved = JSON.parse(readFileSync(target,"utf8"));
    assert.deepEqual(saved.firebase, {apiKey:"test-key",authDomain:"test-project.firebaseapp.com",projectId:"test-project",appId:"test-app"});
    for (const name of ["DOMIOS_FIREBASE_API_KEY","DOMIOS_FIREBASE_PROJECT_ID","DOMIOS_FIREBASE_APP_ID"]) delete env[name];
    const missing = spawnSync(process.execPath, [new URL("./write-signin-config.mjs", import.meta.url).pathname], {env,encoding:"utf8"});
    assert.equal(missing.status, 1);
    assert.match(missing.stderr, /Missing sign-in configuration/);
    assert.throws(() => signinConfig({DOMIOS_FIREBASE_API_KEY:" "}), /DOMIOS_FIREBASE_API_KEY/);
  } finally { rmSync(target, {force:true}); }
});
