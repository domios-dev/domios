// Caller: build.sh dev/check-queue; scripts/dev-build-configuration.test.mjs
import fs from 'node:fs';
import { pathToFileURL } from 'node:url';

function featureSection(manifest) {
  return manifest.match(/^\[features\]\s*\n([\s\S]*?)(?=^\[|$(?![\s\S]))/m)?.[1] ?? '';
}

function defaultFeatureSet(manifest) {
  const defaults = featureSection(manifest).match(/^default\s*=\s*\[([\s\S]*?)\]/m)?.[1] ?? '';
  return [...defaults.matchAll(/"([^"]+)"/g)].map(match => match[1]);
}

function dependencyOnlyFeatures(manifest) {
  return new Set([...featureSection(manifest).matchAll(/^([\w-]+)\s*=\s*\[([\s\S]*?)\]/gm)]
    .filter(([, , definition]) => {
      const members = [...definition.matchAll(/"([^"]+)"/g)].map(match => match[1]);
      return members.length > 0 && members.every(member => member.startsWith('dep:'));
    })
    .map(([, name]) => name));
}

export function devBuildConfiguration(manifest) {
  const features = defaultFeatureSet(manifest).filter(feature => feature !== 'custom-protocol');
  return { profile: 'dev', features, defaultFeatures: false };
}

export function assertServerFeatureConfiguration(server, core) {
  const dev = devBuildConfiguration(server).features.sort();
  const serverDefaults = defaultFeatureSet(server).sort();
  const coreDefaults = defaultFeatureSet(core).sort();
  const serverOnly = dependencyOnlyFeatures(server);
  const coreFeatures = dev.filter(feature => !serverOnly.has(feature));
  if (JSON.stringify(dev) !== JSON.stringify(serverDefaults) || JSON.stringify(coreFeatures) !== JSON.stringify(coreDefaults)) {
    throw new Error(`Server feature configuration diverged: dev=${dev} server-default=${serverDefaults} core-default/test=${coreDefaults}; refusing a second core configuration`);
  }
  return coreFeatures;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv[2] === '--guard') {
    process.stdout.write(assertServerFeatureConfiguration(fs.readFileSync(process.argv[3], 'utf8'), fs.readFileSync(process.argv[4], 'utf8')).join(','));
  } else {
    process.stdout.write(devBuildConfiguration(fs.readFileSync(process.argv[2], 'utf8')).features.join(','));
  }
}
