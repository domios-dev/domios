// Caller: build.sh dev/check-queue; scripts/dev-build-configuration.test.mjs
import fs from 'node:fs';
import { pathToFileURL } from 'node:url';

export function devBuildConfiguration(manifest) {
  const section = manifest.match(/^\[features\]\s*\n([\s\S]*?)(?=^\[|$(?![\s\S]))/m)?.[1];
  const defaults = section?.match(/^default\s*=\s*\[([\s\S]*?)\]/m)?.[1] ?? '';
  const features = [...defaults.matchAll(/"([^"]+)"/g)].map(match => match[1]).filter(feature => feature !== 'custom-protocol');
  return { profile: 'dev', features, defaultFeatures: false };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.stdout.write(devBuildConfiguration(fs.readFileSync(process.argv[2], 'utf8')).features.join(','));
}
