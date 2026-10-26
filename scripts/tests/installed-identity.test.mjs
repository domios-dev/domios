import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { checkInstalledIdentity, identityViolations } from '../check-installed-identity.mjs';
import { assertLegacyRefusal, manualInstallNotes, writeLegacyRefusals } from '../legacy-updater-refusal.mjs';

const read = relative => readFileSync(new URL(`../../${relative}`, import.meta.url), 'utf8');

test('production and development bundle identities select distinct Domios binaries', () => {
  const prod = JSON.parse(read('src-tauri/tauri.conf.json'));
  const dev = JSON.parse(read('src-tauri/tauri.dev.conf.json'));
  assert.deepEqual([prod.productName, prod.identifier, prod.mainBinaryName], ['Domios', 'com.domios.app', 'domios-app']);
  assert.deepEqual([dev.productName, dev.identifier, dev.mainBinaryName], ['Domios Dev', 'com.domios.dev', 'domios-dev-app']);
  assert.equal(prod.bundle.windows.nsis.installMode, 'currentUser');
  assert.deepEqual(prod.plugins['deep-link'].desktop.schemes, ['domios']);
  assert.match(prod.plugins.updater.endpoints[0], /\/domios-latest\.json$/);
  assert.match(read('src-tauri/Info.plist'), /CFBundleDisplayName<\/key>\s*<string>Domios<\/string>/);
});

test('Linux package managers own both CLI commands independently of the GUI', () => {
  const config = JSON.parse(read('src-tauri/tauri.linux.conf.json'));
  for (const kind of ['deb', 'rpm']) {
    assert.deepEqual(config.bundle.linux[kind].files, {
      '/usr/bin/domios': '../target/domios-package/cli/domios',
      '/usr/bin/codeterm': '../target/domios-package/cli/codeterm',
    });
  }
});

test('installed and released identity has no former product name or bundle identifier', () => {
  assert.deepEqual(checkInstalledIdentity(), []);
  assert.equal(identityViolations('tauri.conf.json', '{"productName":"CodeTerm"}').length, 1);
  assert.equal(identityViolations('installer.nsi', 'InstallDir "$LOCALAPPDATA\\CodeTerm"').length, 1);
  assert.equal(identityViolations('manifest.json', '"identifier":"com.codeterm.app"').length, 1);
  assert.deepEqual(identityViolations('internal.rs', 'codeterm_types::Config CODETERM_API_URL rollacode/codeterm-plugins'), []);
});

test('legacy release paths carry manual steps and no installable payload for every transport', () => {
  const directory = mkdtempSync(path.join(os.tmpdir(), 'domios-legacy-updater-'));
  try {
    for (const channel of ['', '-rc', '-nightly']) {
      const current = `domios-latest${channel}.json`;
      const domios = { version: '1.15.2', pub_date: '2026-10-22T00:00:00Z', platforms: { 'windows-x86_64': { url: 'https://example.invalid/Domios-setup.exe', signature: 'signed' } } };
      writeFileSync(path.join(directory, current), JSON.stringify(domios));
      const names = writeLegacyRefusals(directory, current);
      assert.deepEqual(names, [`latest${channel}.json`, `latest${channel}-deb.json`, `latest${channel}-rpm.json`]);
      for (const name of names) {
        const refusal = JSON.parse(readFileSync(path.join(directory, name), 'utf8'));
        assertLegacyRefusal(refusal, domios.version);
        assert.throws(() => assertLegacyRefusal({ ...refusal, platforms: domios.platforms }, domios.version), /must refuse/);
        assert.throws(() => assertLegacyRefusal({ ...refusal, notes: 'Install now' }, domios.version), /manual steps/);
        assert.equal(refusal.version, domios.version);
        assert.equal(refusal.notes, manualInstallNotes);
        assert.deepEqual(refusal.platforms, {});
        assert.equal(JSON.stringify(refusal).includes('signature'), false);
      }
      assert.deepEqual(JSON.parse(readFileSync(path.join(directory, current), 'utf8')), domios);
    }
    assert.throws(() => writeLegacyRefusals(directory, '../latest.json'), /unknown Domios channel/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

