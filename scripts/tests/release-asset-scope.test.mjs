import assert from 'node:assert/strict';
import test from 'node:test';
import { headlessAssetNames } from '../headless-release-package.mjs';
import { ownsReleaseAsset } from '../release-asset-scope.mjs';
const assets = {
  'darwin-aarch64': ['Domios_aarch64.app.tar.gz', 'Domios_1.14.7_aarch64.dmg', 'updater-darwin-aarch64.json'],
  'darwin-x86_64': ['Domios_x64.app.tar.gz', 'Domios_1.14.7_x64.dmg', 'updater-darwin-x86_64.json'],
  'linux-aarch64': ['Domios_1.14.7_aarch64.AppImage', 'Domios_1.14.7_arm64.deb', 'updater-linux-aarch64-deb.json'],
  'linux-x86_64': ['Domios_1.14.7_amd64.AppImage', 'Domios_1.14.7_amd64.deb', 'updater-x86_64-unknown-linux-gnu-deb.json'],
  'windows-x86_64': ['Domios_1.14.7_x64-setup.exe', 'updater-x86_64-pc-windows-msvc.json'],
  'windows-aarch64': ['Domios_1.14.7_arm64-setup.exe', 'updater-aarch64-pc-windows-msvc.json'],
  frontend: ['frontend-v1.14.7.tar.gz', 'frontend-manifest.json', 'frontend-manifest.json.minisig'],
};
test('retry cleanup owns only its artifact names, never another active artifact or the final manifest', () => {
  for (const [platform, names] of Object.entries(assets)) {
    for (const [owner, other] of Object.entries(assets)) {
      for (const name of other) assert.equal(ownsReleaseAsset(name, platform), owner === platform, `${platform}/${name}`);
    }
    for (const name of ['domios-latest.json', 'domios-latest-deb.json', 'issue-closure-boundary.json']) assert.equal(ownsReleaseAsset(name, platform), false);
    assert.ok(names.length > 0);
  }
});
test('unknown platform cleanup is refused', () => assert.throws(() => ownsReleaseAsset('bundle', 'unknown')));

test('headless archive, checksum, and manifest retry cleanup stays platform-scoped', () => {
  for (const platform of Object.keys(assets).filter(name => name !== 'frontend')) {
    const names = Object.values(headlessAssetNames('v1.14.7', platform));
    for (const name of names) {
      assert.equal(ownsReleaseAsset(name, platform), true, platform + '/' + name);
      for (const other of Object.keys(assets).filter(candidate => candidate !== 'frontend' && candidate !== platform)) {
        assert.equal(ownsReleaseAsset(name, other), false, other + '/' + name);
      }
    }
  }
  assert.equal(ownsReleaseAsset('domios-headless-v1.14.7-linux-x86_64.tar.gz.sig', 'linux-x86_64'), false);
});
