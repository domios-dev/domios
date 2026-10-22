import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const manualInstallNotes = readFileSync(new URL('../docs/release/1.15.2-manual-install.md', import.meta.url), 'utf8').trim();

export function assertLegacyRefusal(manifest, version) {
  if (manifest.version !== version || manifest.notes !== manualInstallNotes ||
      !manifest.platforms || Array.isArray(manifest.platforms) || Object.keys(manifest.platforms).length !== 0 ||
      Object.hasOwn(manifest, 'url') || Object.hasOwn(manifest, 'signature')) {
    throw new Error('legacy updater must refuse installation with the manual steps');
  }
}

export function writeLegacyRefusals(directory, manifestName = 'domios-latest.json') {
  if (!/^domios-latest(?:-rc|-nightly)?\.json$/.test(manifestName)) throw new Error('unknown Domios channel');
  const current = JSON.parse(readFileSync(path.join(directory, manifestName), 'utf8'));
  if (!/^\d+\.\d+\.\d+(?:-[\w.-]+)?$/.test(current.version)) throw new Error('invalid release version');
  const refusal = { version: current.version, notes: manualInstallNotes, pub_date: current.pub_date, platforms: {} };
  const legacy = manifestName.replace('domios-', '');
  const names = [legacy, legacy.replace('.json', '-deb.json'), legacy.replace('.json', '-rpm.json')];
  for (const name of names) writeFileSync(path.join(directory, name), JSON.stringify(refusal, null, 2) + '\n');
  return names;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[2] === 'verify') {
    const [url, version] = process.argv.slice(3);
    const endpoint = new URL(url);
    if (endpoint.protocol !== 'https:' || !/\/latest(?:-rc|-nightly)?\.json$/.test(endpoint.pathname)) throw new Error('expected HTTPS legacy manifest URL');
    for (const suffix of ['', '-deb', '-rpm']) {
      const target = new URL(endpoint);
      target.pathname = target.pathname.replace(/\.json$/, `${suffix}.json`);
      const response = await fetch(target, { signal: AbortSignal.timeout(60_000), cache: 'no-store' });
      if (!response.ok) throw new Error(`legacy refusal readback returned HTTP ${response.status}`);
      assertLegacyRefusal(await response.json(), version);
    }
    console.log('legacy updater refusals verified for app, Deb and RPM');
  } else {
    writeLegacyRefusals(process.argv[2], process.argv[3]);
  }
}
