import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import vm from 'node:vm';

const source = readFileSync(new URL('../../packaging/linux/49-domios-update.rules', import.meta.url), 'utf8');

function authorize({ id = 'org.freedesktop.systemd1.manage-units', user = 'domios-server', unit = 'domios-update.service', verb = 'start' } = {}) {
  let rule;
  vm.runInNewContext(source, { polkit: { addRule: candidate => { rule = candidate; }, Result: { YES: 'yes' } } });
  return rule({ id, lookup: key => ({ unit, verb })[key] }, { user });
}

test('headless updater authorizes only the service account starting the fixed root unit', () => {
  assert.equal(authorize(), 'yes');
});

test('headless updater refuses other users, units, verbs and unit-file management', () => {
  for (const input of [
    { user: 'other' }, { unit: 'domios-server.service' }, { unit: 'arbitrary.service' },
    { verb: 'stop' }, { verb: 'restart' }, { verb: 'reload' }, { verb: 'start-replace' },
    { id: 'org.freedesktop.systemd1.manage-unit-files' },
  ]) assert.equal(authorize(input), undefined, JSON.stringify(input));
});
