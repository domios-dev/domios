// Caller: build.sh script-guards; skill build
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { pathToFileURL } from 'node:url';
import readline from 'node:readline';
import test from 'node:test';
const root = path.resolve(import.meta.dirname, '..');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const env = { ...process.env, PATH: process.platform === 'win32' ? `C:/Program Files/Git/usr/bin;C:/Program Files/Git/bin;${path.dirname(process.execPath)};${process.env.PATH}` : process.env.PATH };
function observed(directory, predicate) {
  return new Promise((resolve, reject) => {
    let timer;
    const watcher = fs.watch(directory, { recursive: true }, check);
    function finish(error, value) { clearTimeout(timer); watcher.close(); error ? reject(error) : resolve(value); }
    function check() { try { const value = predicate(); if (value) finish(null, value); } catch (e) { if (e.code !== 'ENOENT') finish(e); } }
    timer = setTimeout(() => finish(new Error('Fixture event deadline exceeded')), 30000);
    check();
  });
}
function shell(directory, body, id = 'fixture') {
  const child = spawn(bash, ['-c', body, 'fixture', directory, id], { cwd: root, env, stdio: ['pipe', 'pipe', 'pipe'] });
  let output = '', errors = '';
  child.stdout.on('data', c => output += c);
  child.stderr.on('data', c => errors += c);
  const lines = readline.createInterface({ input: child.stdout });
  const ready = new Promise(resolve => lines.once('line', resolve));
  const exited = once(child, 'close').then(([code]) => ({ code, output, errors }));
  return { child, ready, exited, id, snapshot: () => ({ output, errors }) };
}
async function cleanup(directory, clients) {
  for (const c of clients) if (c.child.exitCode === null) {
    c.child.stdin.destroy();
    if (process.platform === 'win32') spawnSync('taskkill.exe', ['/PID', String(c.child.pid), '/T', '/F'], { stdio: 'ignore' });
    else c.child.kill('SIGKILL');
  }
  await Promise.all(clients.map(c => c.exited));
  fs.rmSync(directory, { recursive: true, force: true });
}
const statusBody = 'source scripts/build-gateway.sh; ct_gateway_set_fixture_dir "$1/state"; ct_gateway_status';

test('four concurrent cache clients start the server exactly once and leave gateway metadata free', { timeout: 60000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-sccache-'));
  const clients = Array.from({ length: 4 }, (_, i) => shell(directory, `
    source scripts/cache-policy.sh
    source scripts/build-gateway.sh
    ct_gateway_set_fixture_dir "$1/state"
    fixture_dir="$1"; fixture_id="$2"
    ct_verify_target_dir() { :; }
    ct_forbid_mutable_cache_dir() { :; }
    ct_reject_basedir_rewriting() { :; }
    ct_sccache_storage() { CT_SCCACHE_STORAGE=local; }
    ct_sccache_print_location() { :; }
    ct_sccache_server_live() { [ -f "$fixture_dir/live" ]; }
    pgrep() { return 1; }
    powershell.exe() { return 1; }
    sccache() {
      case "$1" in
        --version) echo fixture ;;
        --stop-server) echo stop >> "$fixture_dir/events" ;;
        --start-server)
          mkdir "$fixture_dir/start-claimed" || { echo 'os error 10048' >&2; return 1; }
          echo start >> "$fixture_dir/events"
          echo "$fixture_id" > "$fixture_dir/starting"
          read -r permit
          echo live > "$fixture_dir/live"
          ;;
        --show-stats) echo 'Cache location Local disk: fixture' ;;
      esac
    }
    echo READY
    read -r start
    ct_configure_sccache || exit $?
    echo DONE
  `, `cache-${i}`));
  const observers = [];
  try {
    await Promise.all(clients.map(c => c.ready));
    const starting = observed(directory, () => fs.existsSync(path.join(directory, 'starting')) && fs.readFileSync(path.join(directory, 'starting'), 'utf8').trim());
    for (const c of clients) c.child.stdin.write('start\n');
    const starter = await starting;
    assert.equal(fs.existsSync(path.join(directory, 'state', 'state.lock')), false);
    const observer = shell(directory, statusBody);
    observers.push(observer);
    const status = await observer.exited;
    assert.equal(status.code, 0, status.errors);
    assert.equal(JSON.parse(status.output).state, 'idle');
    clients.find(c => c.id === starter).child.stdin.end('permit\n');
    const results = await Promise.all(clients.map(c => c.exited));
    for (const result of results) {
      assert.equal(result.code, 0, result.errors);
      assert.match(result.output, /DONE/);
      assert.doesNotMatch(result.errors, /10048|EPERM|Timed out/);
    }
    assert.deepEqual(fs.readFileSync(path.join(directory, 'events'), 'utf8').trim().split(/\r?\n/), ['stop', 'start']);
  } finally { await cleanup(directory, [...clients, ...observers]); }
});

test('cache retention holds its own mutex while gateway metadata stays available', { timeout: 45000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-retention-'));
  const holder = shell(directory, `
    source scripts/cache-policy.sh
    source scripts/build-gateway.sh
    ct_gateway_set_fixture_dir "$1/state"
    node() {
      case "$1" in
        */cache-retention.mjs) echo HOLDING; read -r permit ;;
        *) command node "$@" ;;
      esac
    }
    ct_cache_retention auto
  `);
  const observers = [];
  try {
    assert.equal(await holder.ready, 'HOLDING');
    assert.equal(fs.existsSync(path.join(directory, 'state', 'cache-retention-state', 'state.lock')), true);
    assert.equal(fs.existsSync(path.join(directory, 'state', 'state.lock')), false);
    const observer = shell(directory, statusBody);
    observers.push(observer);
    const result = await observer.exited;
    assert.equal(result.code, 0, result.errors);
    assert.equal(JSON.parse(result.output).state, 'idle');
    holder.child.stdin.end('permit\n');
    assert.equal((await holder.exited).code, 0);
  } finally { await cleanup(directory, [holder, ...observers]); }
});

test('gateway helpers stay pinned to the launching scripts after checkout cwd changes', { timeout: 20000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-helper-root-'));
  const client = shell(directory, `
    source scripts/build-gateway.sh
    mkdir -p "$1/state"
    cd "$1"
    ct_repo_root() { pwd; }
    ct_gateway_mutex_acquire "$1/state" || exit $?
    ct_gateway_mutex_release "$1/state" || exit $?
    ct_gateway_set_fixture_dir "$1/state"
    ct_gateway_status
  `);
  try {
    const result = await client.exited;
    assert.equal(result.code, 0, result.errors);
    assert.equal(JSON.parse(result.output).state, 'idle');
  } finally { await cleanup(directory, [client]); }
});

test('cache reset metadata failure stops server restart before compile', { timeout: 15000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-cache-evidence-'));
  const client = shell(directory, `
    source scripts/cache-policy.sh
    source scripts/build-gateway.sh
    ct_gateway_set_fixture_dir "$1/state"
    CT_GATEWAY_ACTIVE=1
    CT_BUILD_JOB_ID=reset-failure
    CT_SCCACHE_STORAGE=local
    pgrep() { return 1; }
    powershell.exe() { return 1; }
    ct_gateway_state_tool() { return 1; }
    sccache() { echo STOPPED; }
    ct_sccache_start_with_evidence() { echo STARTED; }
    if ct_sccache_restart; then echo COMPILE_STARTED; else exit "$?"; fi
  `);
  try {
    const result = await client.exited;
    assert.equal(result.code, 74, result.errors);
    assert.doesNotMatch(result.output, /STOPPED|STARTED/);
    assert.match(result.errors, /metadata write failed before compilation: cache reset evidence/);
    assert.equal(fs.existsSync(path.join(directory, 'state', 'sccache-start-state', 'state.lock')), false);
  } finally { await cleanup(directory, [client]); }
});

test('mixed build verbs share one cache startup and persist four completed history rows', { timeout: 60000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-mixed-verbs-'));
  const source = fs.readFileSync(path.join(root, 'build.sh'), 'utf8');
  const dispatch = '\ncase "$CMD" in\n  fmt-check|';
  assert.equal(source.split('ct_configure_sccache\n').length - 1, 1);
  assert.ok(source.includes(dispatch));
  const fixture = `
    fixture_dir="$MIXED_FIXTURE_DIR"
    ct_gateway_set_fixture_dir "$fixture_dir/state"
    ct_cache_retention() { :; }
    enter_stable_build_path() { :; }
    prepare_dev_frontend_dist() { :; }
    prepare_bundle_stubs() { :; }
    check_queue_worktree() { pwd; }
    git() {
      if [ "\${1:-}" = -C ] && [ "\${3:-}" = checkout ]; then return 0; fi
      command git "$@"
    }
    ct_verify_target_dir() { :; }
    ct_forbid_mutable_cache_dir() { :; }
    ct_reject_basedir_rewriting() { :; }
    ct_sccache_storage() { CT_SCCACHE_STORAGE=local; }
    ct_sccache_print_location() { :; }
    ct_sccache_server_live() { [ -f "$fixture_dir/live" ]; }
    pgrep() { return 1; }
    powershell.exe() { return 1; }
    sccache() {
      case "$1" in
        --version) echo fixture ;;
        --stop-server) echo stop >> "$fixture_dir/events" ;;
        --start-server)
          mkdir "$fixture_dir/start-claimed" || { echo 'os error 10048' >&2; return 1; }
          echo start >> "$fixture_dir/events"
          echo "$CT_BUILD_JOB_ID" > "$fixture_dir/starting"
          read -r permit
          echo live > "$fixture_dir/live"
          ;;
        --show-stats) echo 'Cache location Local disk: fixture' ;;
      esac
    }
    cargo() {
      [ "$CT_GATEWAY_ACTIVE" = 1 ] && [ -f "$fixture_dir/live" ] || return 98
      echo "$CT_BUILD_JOB_ID $*" >> "$fixture_dir/cargo"
      if [ "$1" = metadata ]; then
        command node -e 'const path=require("node:path"); const names=["codeterm","codeterm-service-core","codeterm-plan"]; console.log(JSON.stringify({workspace_members:names,packages:names.map(name=>({id:name,name,manifest_path:path.join(process.cwd(),"src-tauri",...(name==="codeterm"?[]:["crates",name]),"Cargo.toml"),targets:[{kind:["lib"]}]}))}))'
      else echo '{}'; fi
    }
    scripts/test-capped.sh() { cargo test "$@"; }
    node() {
      if [ "$1" = scripts/check-slow-unit-tests.mjs ]; then
        case "$*" in *--candidates*) echo '{"entries":[]}' > "$4" ;; esac
        return 0
      fi
      command node "$@"
    }
    echo READY
    read -r start
  `;
  const identityPreload = path.join(directory, 'fixture-identity.mjs');
  fs.writeFileSync(identityPreload, `
    import cp from 'node:child_process';
    import { syncBuiltinESMExports } from 'node:module';
    const spawn = cp.spawnSync;
    cp.spawnSync = function(command, args, ...rest) {
      const pid = command === 'powershell.exe' && args.join(' ').match(/GetProcessById\\((\\d+)\\)/)?.[1];
      if (!pid) return spawn.call(this, command, args, ...rest);
      let present = true;
      try { process.kill(Number(pid), 0); } catch { present = false; }
      return { status: 0, stdout: present ? 'fixture-' + pid : 'GONE', stderr: '' };
    };
    syncBuiltinESMExports();
  `);
  const fixtureScript = path.join(directory, 'build.sh');
  fs.writeFileSync(fixtureScript, source.replace('*) setup_env ;;', '*) : ;;').replace(dispatch, fixture + dispatch));
  const verbs = [['test-one', 'plan', 'fixture'], ['test-one', 'cli', 'fixture'], ['nextest', 'plan'], ['check-queue', '--all']];
  const clients = verbs.map((args, i) => shell(directory, `
    export NODE_OPTIONS="--import=${pathToFileURL(identityPreload).href}"
    export MIXED_FIXTURE_DIR="$1" CT_BUILD_JOB_ID="$2" CT_BUILD_GATEWAY_NO_TEE=1 CODETERM_BUILD_GATEWAY_MODE=queue
    unset CODETERM_BUILD_GATEWAY_FIXTURE_DIR CT_BUILD_GATEWAY_SCOPE BUILD_NOTIFY_TAB
    source "$1/build.sh" ${args.join(' ')}
  `, `mixed-${i}`));
  try {
    await Promise.all(clients.map(c => c.ready));
    const starting = observed(directory, () => fs.existsSync(path.join(directory, 'starting')) && fs.readFileSync(path.join(directory, 'starting'), 'utf8').trim());
    for (const c of clients) c.child.stdin.write('start\n');
    const starter = await Promise.race([starting, ...clients.map(async client => {
      const result = await client.exited;
      throw new Error(`${client.id} exited before startup: ${result.code}\n${result.output}\n${result.errors}`);
    })]);
    await Promise.race([observed(directory, () => {
      const jobs = path.join(directory, 'state', 'jobs');
      return fs.readdirSync(jobs).filter(name => name.endsWith('.json')).length === 4;
    }), ...clients.map(async client => {
      const result = await client.exited;
      throw new Error(`${client.id} exited before all jobs registered: ${result.code}\n${result.output}\n${result.errors}`);
    })]);
    clients.find(c => c.id === starter).child.stdin.end('permit\n');
    const results = await Promise.all(clients.map(c => c.exited));
    for (const result of results) {
      assert.equal(result.code, 0, result.output + result.errors);
      assert.doesNotMatch(result.errors, /10048|EPERM|Timed out/);
    }
    assert.deepEqual(fs.readFileSync(path.join(directory, 'events'), 'utf8').trim().split(/\r?\n/), ['stop', 'start']);
    const history = spawnSync(process.execPath, ['scripts/build-history.mjs', '--dir', path.join(directory, 'state'), '--json'], { cwd: root, env, encoding: 'utf8' });
    assert.equal(history.status, 0, history.stderr);
    const rows = JSON.parse(history.stdout).jobs;
    assert.equal(rows.length, 4);
    assert.deepEqual(rows.map(row => row.verb).sort(), ["check-queue", "nextest", "test-one", "test-one"]);
    for (const row of rows) {
      assert.equal(row.state, "completed", JSON.stringify(row));
      assert.equal(row.exit_code, 0, JSON.stringify(row));
    }
    assert.equal(new Set(fs.readFileSync(path.join(directory, 'cargo'), 'utf8').trim().split(/\r?\n/).map(line => line.split(' ')[0])).size, 4);
  } catch (error) {
    const jobs = path.join(directory, 'state', 'jobs');
    const records = fs.existsSync(jobs) ? fs.readdirSync(jobs).filter(name => name.endsWith('.json')).map(name => {
      try { const row = JSON.parse(fs.readFileSync(path.join(jobs, name), 'utf8')); return { id: row.job_id, state: row.state }; }
      catch { return { id: name, state: 'unreadable' }; }
    }) : [];
    throw new Error(`${error.message}\n${JSON.stringify({ records, files: fs.readdirSync(directory), clients: clients.map(c => ({ id: c.id, ...c.snapshot() })) })}`);
  } finally { await cleanup(directory, clients); }
});

test('every cargo dispatch is covered by the common entry', () => {
  const source = fs.readFileSync(path.join(root, 'build.sh'), 'utf8');
  const centralized = source.match(/case "\$CMD" in\n  (fmt-check\|[^\n]+)\)\n    enter_cargo_gateway/)[1].split('|');
  const dispatch = source.slice(source.indexOf('\ncase "$CMD" in\n  help)'));
  const branches = [...dispatch.matchAll(/^  ([\w:|-]+)\)\n([\s\S]*?)(?=^  [\w:|-]+\)\n|$(?![\s\S]))/gm)];
  for (const [, names, body] of branches) {
    const executable = body.replace(/cat <<'EOF'[\s\S]*?^EOF$/gm, "");
    if (!/\bcargo (?:build|check|clippy|clean|fmt|test)|scripts\/test-capped\.sh|run_workspace_lib_tests|prepare_release_bundle_inputs|ci-rust-tests/.test(executable)) continue;
    for (const name of names.split('|')) {
      assert.ok(centralized.includes(name) || body.includes('enter_cargo_gateway "$CMD'), `${name} bypasses the cargo gateway entry`);
    }
  }
  assert.equal((source.match(/^\s*ct_configure_sccache$/gm) ?? []).length, 1);
  const archives = fs.readFileSync(path.join(root, 'scripts/nextest-archive.sh'), 'utf8');
  assert.match(archives, /enter_cargo_gateway "nextest-archive \$sha"[\s\S]*cargo nextest archive/);
});

test('fixture and foreign jobs cannot map or release the production stable drive', { timeout: 15000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-stable-drive-'));
  const source = fs.readFileSync(path.join(root, 'build.sh'), 'utf8');
  const functions = ['enter_stable_build_path', 'release_stable_build_path'].map(name =>
    source.match(new RegExp(`^${name}\\(\\) \\{[\\s\\S]*?^\\}`, 'm'))[0]).join('\n');
  const ownership = fs.readFileSync(path.join(root, 'scripts/build-gateway.sh'), 'utf8')
    .match(/^ct_gateway_owned_by_caller\(\) \{[\s\S]*?^\}/m)[0];
  const client = shell(directory, ownership + "\n" + functions + `
    fixture_dir="$1"
    ct_gateway_dir() { printf '%s\\n' "$fixture_dir/state"; }
    subst() { echo "$*" >> "$fixture_dir/mappings"; }
    cygpath() { echo fixture-path; }
    cd() { if [ "$1" = /x ]; then return 0; fi; builtin cd "$@"; }
    OSTYPE=msys
    mkdir -p "$fixture_dir/state/lock"
    CT_BUILD_GATEWAY_SCOPE=fixture
    CT_GATEWAY_ACTIVE=1
    CT_BUILD_JOB_ID=fixture
    CT_STABLE_BUILD_DRIVE_MAPPED=X
    enter_stable_build_path
    release_stable_build_path
    [ ! -e "$fixture_dir/mappings" ] || exit 98
    CT_BUILD_GATEWAY_SCOPE=production
    if enter_stable_build_path; then exit 97; else [ "$?" = 75 ] || exit 96; fi
    [ ! -e "$fixture_dir/mappings" ] || exit 95
    echo production > "$fixture_dir/state/lock/job_id"
    echo "$$" > "$fixture_dir/state/lock/pid"
    echo fixture-start > "$fixture_dir/state/lock/start_token"
    CT_BUILD_JOB_ID=production
    CT_GATEWAY_PROCESS_START=fixture-start
    enter_stable_build_path
    echo successor > "$fixture_dir/state/lock/job_id"
    release_stable_build_path
    [ "$(wc -l < "$fixture_dir/mappings")" = 2 ] || exit 94
    echo production > "$fixture_dir/state/lock/job_id"
    release_stable_build_path
    [ "$(wc -l < "$fixture_dir/mappings")" = 3 ] || exit 93
  `);
  try {
    const result = await client.exited;
    assert.equal(result.code, 0, result.output + result.errors);
    assert.match(result.errors, /exact production gateway owner/);
  } finally { await cleanup(directory, [client]); }
});
