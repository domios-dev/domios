import test from "node:test";
import assert from "node:assert/strict";

const healthReady = (health, apiPort) =>
  health?.status === "ok" && String(health.instance?.api_port) === String(apiPort);

const pollFixture = (observations, apiPort) => {
  for (const observation of observations) {
    if (observation.exitCode !== undefined) {
      throw new Error(`dev process exited before readiness (code=${observation.exitCode})`);
    }
    if (healthReady(observation.health, apiPort)) return observation.health;
  }
  throw new Error("readiness deadline exceeded");
};

test("dev_wait_health accepts already-present healthy dev health", () => {
  const health = pollFixture([{ health: { status: "ok", instance: { api_port: 7686 } } }], 7686);
  assert.equal(health.status, "ok");
});

test("dev_wait_health observes eventual health in the detector sequence", () => {
  const health = pollFixture([
    { health: { status: "starting", instance: { api_port: 7686 } } },
    { health: { status: "ok", instance: { api_port: 7686 } } },
  ], 7686);
  assert.equal(health.instance.api_port, 7686);
});

test("dev_wait_health reports failed readiness with the real non-zero exit status", () => {
  assert.throws(
    () => pollFixture([{ exitCode: 17 }], 7686),
    /exited before readiness \(code=17\)/,
  );
});

test("dev_wait_health rejects prod health while scoping to the dev port", () => {
  assert.throws(
    () => pollFixture([{ health: { status: "ok", instance: { api_port: 7685 } } }], 7686),
    /readiness deadline exceeded/,
  );
});

test("DEV web watcher enables Vite diagnostics on both platform branches", async () => {
  const { readFile } = await import("node:fs/promises");
  const source = await readFile(new URL("../../build.sh", import.meta.url), "utf8");
  const watch = source.match(/^start_web_watch\(\) \{\n([\s\S]*?)^\}/m)?.[1];
  assert.ok(watch, "DEV web watcher must exist");
  const commands = watch.split("\n").filter(line => line.includes('"$vite_entry"'));
  assert.equal(commands.length, 2, "Unix and Windows watcher commands must be covered");
  for (const command of commands) {
    assert.match(command, /NODE_ENV=development CODETERM_FRONTEND_DEV=1 (?:nohup )?node "\$vite_entry" build --mode development --watch/);
  }
  assert.doesNotMatch(source, /^\s*export\s+(?:NODE_ENV=development|CODETERM_FRONTEND_DEV=1)/m);
});

test("release frontend preparation retains the default production build", async () => {
  const { readFile } = await import("node:fs/promises");
  const source = await readFile(new URL("../../build.sh", import.meta.url), "utf8");
  const release = source.match(/^prepare_release_bundle_inputs\(\) \{\n([\s\S]*?)^\}/m)?.[1];
  assert.ok(release, "release input preparation must exist");
  assert.match(release, /^  pnpm build$/m);
  assert.doesNotMatch(release, /NODE_ENV|CODETERM_FRONTEND_DEV|--mode development/);
  const pkg = JSON.parse(await readFile(new URL("../../package.json", import.meta.url), "utf8"));
  assert.equal(pkg.scripts.build, "node node_modules/typescript/bin/tsc && node node_modules/vite/bin/vite.js build");
});
