#!/usr/bin/env node
// Caller: CI release-linux-arm64.yml, release-macos-build.yml; scripts tests/test_release_workflow_contract.py
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, readdirSync, copyFileSync } from 'node:fs';
import path from 'node:path';

const [tag, platform, directory] = process.argv.slice(2);
if (!/^v\d+\.\d+\.\d+$/.test(tag ?? '') || !/^(darwin-(aarch64|x86_64)|linux-aarch64)$/.test(platform ?? '')) throw new Error('Invalid platform publication');
const version = tag.slice(1);
const entries = readdirSync(directory);
let bundle;
let assets;
let deb;
if (platform.startsWith('darwin-')) {
  const arch = platform === 'darwin-aarch64' ? 'aarch64' : 'x64';
  bundle = `CodeTerm_${arch}.app.tar.gz`;
  for (const suffix of ['', '.sig']) copyFileSync(path.join(directory, `CodeTerm_${version}_${arch}.app.tar.gz${suffix}`), path.join(directory, bundle + suffix));
  assets = [bundle, `CodeTerm_${version}_${arch}.dmg`];
} else {
  bundle = entries.find(name => name.endsWith('_aarch64.AppImage'));
  deb = entries.find(name => name.endsWith('_arm64.deb'));
  if (!bundle || !deb) throw new Error('Missing Linux arm64 artifacts');
  assets = [bundle, deb];
}
const fragment = {
  version, notes: `Domios ${version}`, pub_date: new Date().toISOString(),
  platforms: { [platform]: {
    url: `https://github.com/rollacode/codeterm-releases/releases/download/${tag}/${bundle}`,
    signature: readFileSync(path.join(directory, `${bundle}.sig`), 'utf8').trim(),
  } },
};
const output = path.join(directory, `updater-${platform}.json`);
writeFileSync(output, JSON.stringify(fragment));
execFileSync(process.execPath, ['scripts/verify-updater-manifest.mjs', '--manifest', output, '--local', directory, '--require-version', version, '--require-platform', platform], { stdio: 'inherit' });
const fragments = [output];
if (deb) {
  const debFragment = structuredClone(fragment);
  debFragment.platforms[platform].url = debFragment.platforms[platform].url.replace(bundle, deb);
  debFragment.platforms[platform].signature = readFileSync(path.join(directory, `${deb}.sig`), 'utf8').trim();
  const debOutput = path.join(directory, `updater-${platform}-deb.json`);
  writeFileSync(debOutput, JSON.stringify(debFragment));
  execFileSync(process.execPath, ['scripts/verify-updater-manifest.mjs', '--manifest', debOutput, '--local', directory, '--require-version', version, '--require-platform', platform], { stdio: 'inherit' });
  fragments.push(debOutput);
}
if (process.env.RELEASE_VERIFY_ONLY !== 'true') for (const repo of ['rollacode/codeterm', ...(process.env.PUBLISH_PUBLIC_MIRROR === 'true' ? ['rollacode/codeterm-releases'] : [])]) {
  const env = { ...process.env, GH_TOKEN: repo.endsWith('-releases') ? process.env.MIRROR_TOKEN : process.env.GH_TOKEN };
  execFileSync('gh', ['release', 'upload', tag, ...assets.map(name => path.join(directory, name)), ...(repo.endsWith('-releases') ? [] : fragments), '--clobber', '--repo', repo], { env, stdio: 'inherit', timeout: 300000 });
}
