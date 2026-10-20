// Caller: build.sh dev/check-queue; scripts/dev-build-configuration.test.mjs
import fs from 'node:fs';
import { pathToFileURL } from 'node:url';

function defaultFeatureSet(manifest) {
  const section = manifest.match(/^\[features\]\s*\n([\s\S]*?)(?=^\[|$(?![\s\S]))/m)?.[1];
  const defaults = section?.match(/^default\s*=\s*\[([\s\S]*?)\]/m)?.[1] ?? '';
  return [...defaults.matchAll(/"([^"]+)"/g)].map(match => match[1]);
}

export function devBuildConfiguration(manifest) {
  const features = defaultFeatureSet(manifest).filter(feature => feature !== 'custom-protocol');
  return { profile: 'dev', features, defaultFeatures: false };
}

export function assertServerFeatureConfiguration(server, core) {
  const dev = devBuildConfiguration(server).features.sort();
  const serverDefaults = defaultFeatureSet(server).sort();
  const coreDefaults = defaultFeatureSet(core).sort();
  if (JSON.stringify(dev) !== JSON.stringify(serverDefaults) || JSON.stringify(dev) !== JSON.stringify(coreDefaults)) {
    throw new Error(`Server feature configuration diverged: dev=${dev} server-default=${serverDefaults} core-default/test=${coreDefaults}; refusing a second core configuration`);
  }
  return dev;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv[2] === '--guard') {
    process.stdout.write(assertServerFeatureConfiguration(fs.readFileSync(process.argv[3], 'utf8'), fs.readFileSync(process.argv[4], 'utf8')).join(','));
  } else {
    process.stdout.write(devBuildConfiguration(fs.readFileSync(process.argv[2], 'utf8')).features.join(','));
  }
}
