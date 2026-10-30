import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { adoptLegacyTarget, refusal, runningFromTarget, stopCommand } from '../dev-named-target.mjs';

function fixture() {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'dev-shared-target-')));
  const checkout = path.join(root, 'checkout'), home = path.join(root, 'home');
  fs.mkdirSync(path.join(checkout, 'src-tauri/target'), { recursive: true });
  fs.mkdirSync(home);
  const legacy = name => {
    const directory = path.join(checkout, 'src-tauri/target/dev-instances', name);
    fs.mkdirSync(path.join(directory, 'debug/incremental'), { recursive: true });
    fs.writeFileSync(path.join(directory, 'debug/marker'), name);
    return directory;
  };
  const instance = (name, port, target, built) => {
    const data = path.join(home, `.domios-dev-${name}`);
    fs.mkdirSync(data, { recursive: true });
    const file = path.join(data, 'dev-build-provenance.json');
    fs.writeFileSync(file, JSON.stringify({ schema: 3, instance: { name, apiPort: String(port), identifier: `com.domios.dev.${name}` },
      artifacts: { server: { path: path.join(target, 'debug/domios-server') } } }));
    if (built) fs.utimesSync(file, built, built);
    return data;
  };
  return { root, checkout, home, legacy, instance, shared: path.join(checkout, 'src-tauri/target/named-dev') };
}

const health = live => async url => {
  const port = Number(new URL(url).port);
  const match = live.find(row => row.port === port);
  if (!match) throw Error('connection refused');
  return { ok: true, json: async () => ({ instance: { name: match.name, api_port: String(port), identifier: `com.domios.dev.${match.name}` } }) };
};

test('adoption renames the single legacy target once and never copies or deletes', async t => {
  const { root, checkout, home, legacy, shared } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const source = legacy('qa8');
  const logs = [];
  const adopted = await adoptLegacyTarget(checkout, home, { log: line => logs.push(line), request: health([]) });
  assert.deepEqual(adopted, { from: source, to: shared });
  assert.equal(fs.readFileSync(path.join(shared, 'debug/marker'), 'utf8'), 'qa8');
  assert.equal(fs.existsSync(source), false);
  assert.match(logs.join('\n'), /adopted .*dev-instances\/qa8 as .*named-dev/);
  legacy('later');
  assert.equal(await adoptLegacyTarget(checkout, home, { log: () => {}, request: health([]) }), null);
  assert.equal(fs.readFileSync(path.join(checkout, 'src-tauri/target/dev-instances/later/debug/marker'), 'utf8'), 'later');
});

test('adoption among several legacy targets takes the newest provenance and leaves the others', async t => {
  const { root, checkout, home, legacy, instance, shared } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const older = legacy('older'), newer = legacy('newer'), orphan = legacy('orphan');
  instance('older', 17801, older, new Date('2026-10-01T00:00:00Z'));
  instance('newer', 17802, newer, new Date('2026-10-20T00:00:00Z'));
  await adoptLegacyTarget(checkout, home, { log: () => {}, request: health([]) });
  assert.equal(fs.readFileSync(path.join(shared, 'debug/marker'), 'utf8'), 'newer');
  for (const directory of [older, orphan]) assert.equal(fs.existsSync(directory), true);
});

test('adoption refuses without moving while a DEV still runs from the selected legacy target', async t => {
  const { root, checkout, home, legacy, instance, shared } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const source = legacy('qa8');
  instance('qa8', 17803, source);
  await assert.rejects(adoptLegacyTarget(checkout, home, { log: () => {}, request: health([{ name: 'qa8', port: 17803 }]) }),
    /qa8 \(api 17803.*\n.*\n.*CODETERM_DEV_INSTANCE='qa8' CODETERM_DEV_API_PORT='17803' \.\/build\.sh kill/s);
  assert.equal(fs.existsSync(source), true);
  assert.equal(fs.existsSync(shared), false);
});

test('several legacy targets without provenance are left alone', async t => {
  const { root, checkout, home, legacy, shared } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  legacy('one'); legacy('two');
  const logs = [];
  assert.equal(await adoptLegacyTarget(checkout, home, { log: line => logs.push(line), request: health([]) }), null);
  assert.equal(fs.existsSync(shared), false);
  assert.match(logs[0], /not adopting: 2 targets/);
});

test('running DEVs are those whose own provenance names the target and whose port answers as that instance', async t => {
  const { root, home, instance, shared } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const elsewhere = path.join(root, 'other-checkout/src-tauri/target/named-dev');
  const alpha = instance('alpha', 17811, shared);
  instance('beta', 17812, shared);
  instance('gamma', 17813, shared);
  instance('foreign', 17814, elsewhere);
  const live = [{ name: 'alpha', port: 17811 }, { name: 'squatter', port: 17812 }, { name: 'foreign', port: 17814 }];
  const running = await runningFromTarget(shared, home, { request: health(live) });
  assert.deepEqual(running.map(row => [row.name, row.port, row.data]), [['alpha', 17811, alpha]]);
  const message = refusal(shared, 'rebuild (dev)', running);
  assert.match(message, /refusing rebuild \(dev\)/);
  assert.match(message, new RegExp(`alpha \\(api 17811, data ${alpha.replaceAll('\\', '\\\\')}\\)`));
  assert.ok(message.includes(stopCommand(running[0])));
  assert.equal(refusal(shared, 'rebuild (dev)', []), null);
});
