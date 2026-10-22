import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { healthReady as serverReady } from "../server-readiness.mjs";

const healthReady = (health, apiPort) =>
  serverReady(health) && String(health.instance?.api_port) === String(apiPort);

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
  const health = pollFixture([{ health: { status: "ok", startup: { state: "ready" }, instance: { api_port: 7686 } } }], 7686);
  assert.equal(health.status, "ok");
});

test("dev_wait_health observes eventual health in the detector sequence", () => {
  const health = pollFixture([
    { health: { status: "starting", instance: { api_port: 7686 } } },
    { health: { status: "ok", startup: { state: "starting" }, instance: { api_port: 7686 } } },
    { health: { status: "ok", startup: { state: "ready" }, instance: { api_port: 7686 } } },
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

test("DEV web watcher keeps a production-mode bundle with the DEV build flag on both platform branches", async () => {
  const { readFile } = await import("node:fs/promises");
  const source = (await readFile(new URL("../../build.sh", import.meta.url), "utf8")).replaceAll("\r\n", "\n");
  const watch = source.match(/^start_web_watch\(\) \{\n([\s\S]*?)^\}/m)?.[1];
  assert.ok(watch, "DEV web watcher must exist");
  const commands = watch.split("\n").filter(line => line.includes('"$vite_entry"'));
  assert.equal(commands.length, 2, "Unix and Windows watcher commands must be covered");
  for (const command of commands) {
    assert.match(command, /CODETERM_FRONTEND_DEV=1 (?:nohup )?node "\$vite_entry" build --watch/);
    assert.doesNotMatch(command, /--mode development|NODE_ENV=development/);
  }
  assert.doesNotMatch(source, /^\s*export\s+(?:NODE_ENV=development|CODETERM_FRONTEND_DEV=1)/m);
});

test("release frontend preparation retains the default production build", async () => {
  const { readFile } = await import("node:fs/promises");
  const source = (await readFile(new URL("../../build.sh", import.meta.url), "utf8")).replaceAll("\r\n", "\n");
  const release = source.match(/^prepare_release_bundle_inputs\(\) \{\n([\s\S]*?)^\}/m)?.[1];
  assert.ok(release, "release input preparation must exist");
  assert.match(release, /^ {2}if \[ -n "\$\{CODETERM_FRONTEND_ARCHIVE:-\}" \]; then\n {4}node scripts\/release-frontend-dist\.mjs install [^\n]+\n {2}else\n {4}pnpm build\n {2}fi$/m);
  assert.doesNotMatch(release, /NODE_ENV|CODETERM_FRONTEND_DEV|--mode development/);
  const pkg = JSON.parse(await readFile(new URL("../../package.json", import.meta.url), "utf8"));
  assert.equal(pkg.scripts.build, "node scripts/frontend-build.mjs");
  const wrapper = await readFile(new URL("../frontend-build.mjs", import.meta.url), "utf8");
  assert.match(wrapper, /'node_modules\/typescript\/bin\/tsc'/);
  assert.match(wrapper, /'node_modules\/vite\/bin\/vite\.js'\), 'build'/);
  assert.doesNotMatch(wrapper, /NODE_ENV|CODETERM_FRONTEND_DEV|--mode/);
});


const buildSource = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const healthFunctions = ['dev_health_fields', 'validate_dev_health'].map(name => buildSource.match(new RegExp(`^${name}\\(\\) \\{[^]*?^\\}`, 'm'))[0]).join('\n');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
for (const role of ['server', 'combined']) {
  test(`actual DEV health validator preserves exact server identity for ${role}`, () => {
    const sha = 'a'.repeat(40);
    const health = { status: 'ok', startup: { state: 'ready' }, build_sha: sha, instance: { name: 'fixture', api_port: '7786',
      ...(role === 'combined' ? { vite_port: '1420', cdp_port: '9223', identifier: 'fixture.dev' } : {}) } };
    const check = value => spawnSync(bash, ['-c', `${healthFunctions}\nvalidate_dev_health "$@"`, '--',
      sha, 'fixture', '7786', '1420', '9223', 'fixture.dev', JSON.stringify(value), role], { encoding: 'utf8' });
    assert.equal(check(health).status, 0);
    for (const value of [
      { ...health, status: 'starting' }, { ...health, build_sha: 'b'.repeat(40) }, { ...health, build_sha: sha.slice(0, 8) },
      { ...health, startup: { state: 'starting' } }, { ...health, startup: { state: 'failed' } }, { ...health, startup: undefined },
      { ...health, instance: { ...health.instance, name: 'other' } },
      { ...health, instance: { ...health.instance, api_port: '7685' } },
    ]) assert.notEqual(check(value).status, 0);
    if (role === 'combined') {
      for (const field of ['vite_port', 'cdp_port', 'identifier']) {
        assert.notEqual(check({ ...health, instance: { ...health.instance, [field]: '' } }).status, 0);
      }
    }
  });
}


test('split reporter requires live authenticated attachment proof in addition to server health', () => {
  const reporter = buildSource.match(/^dev_notify_health_proof\(\) \{[\s\S]*?^\}/m)[0];
  for (const admitted of [false, true]) {
    const result = spawnSync(bash, ['-c', `set -eu
${reporter}
EXPECTED_BUILD_SHA=${'a'.repeat(40)} CT_SPLIT_DEV=1 CT_INSTANCE=fixture CT_PORT=7786
CT_VITE_PORT=1420 CT_CDP_PORT=9223 CT_IDENTIFIER=fixture.dev
DEV_NOTIFY_LOG=owned.log DEV_NOTIFY_SENT_MARKER=marker
validate_dev_health() { [ "$8" = server ] && printf '%s' "$1"; }
dev_health_status() { printf '{}'; }
dev_listener_pid() { printf 20; }
node() {
  [ "$*" = 'scripts/dev-prep.mjs --check-attachment --log owned.log --api-port 7786 --listener-pid 20' ] || return 91
  return ${admitted ? 0 : 42}
}
dev_notify_ready_after_proof() { printf 'reported:%s:%s' "$5" "$6"; }
dev_notify_health_proof parent
`], { encoding: 'utf8' });
    assert.equal(result.status, admitted ? 0 : 1, result.stderr);
    if (admitted) assert.equal(result.stdout, `reported:${'a'.repeat(40)}:20`);
    else assert.equal(result.stdout, '');
  }
});


test('bench startup waits on the launcher acknowledgement before startup events', () => {
  const lib = readFileSync(new URL('../../evals/_lib/lib.sh', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
  const functions = ['dev_wait_ready', 'dev_start_bg'].map(name => lib.match(new RegExp(`^${name}\\(\\) \\{[^]*?^\\}`, 'm'))[0]).join('\n');
  const directory = mkdtempSync(path.join(tmpdir(), 'bench-startup-'));
  try {
    for (const exit of [0, 17]) {
      const result = spawnSync(bash, ['-c', `set -eu
${functions}
EVAL_LIB_REPO_ROOT="$PWD"
dev_data_dir() { printf '%s' "$STARTUP_FIXTURE_DIR"; }
dev_api_url() { printf 'http://127.0.0.1:7786'; }
dev_run() {
  if [ "$1" = node ]; then
    [ "$3" = wait ] && [ "$4" = http://127.0.0.1:7786 ] || return 91
    printf 'startup-events'
  else
    [ "$2 $3 $4" = 'build.sh dev --wait' ] || return 92
    return ${exit}
  fi
}
dev_start_bg
dev_wait_ready 10
`], { encoding: 'utf8', env: { ...process.env, STARTUP_FIXTURE_DIR: directory } });
      assert.equal(result.status, exit === 0 ? 0 : 1, result.stderr);
      assert.equal(result.stdout, exit === 0 ? 'startup-events' : '');
    }
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test('bench startup refuses an acknowledgement descriptor owned by its caller', () => {
  const lib = readFileSync(new URL('../../evals/_lib/lib.sh', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
  const start = lib.match(/^dev_start_bg\(\) \{[^]*?^\}/m)[0];
  const result = spawnSync(bash, ['-c', `${start}\nexec 9</dev/null\ndev_start_bg`], { encoding: 'utf8' });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /already in use/);
});
