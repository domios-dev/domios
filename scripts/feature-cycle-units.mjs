// Caller: build.sh feature-cycle; scripts/tests/feature-cycle-units.test.mjs
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export function coreUnits(logs) {
  const units = new Map();
  for (const [phase, log] of Object.entries(logs)) {
    for (const line of log.split('\n')) {
      let item;
      try { item = JSON.parse(line); } catch { continue; }
      if (item.reason !== 'compiler-artifact' || item.target?.name !== 'codeterm_service_core') continue;
      for (const file of item.filenames ?? []) {
        const hash = file.replaceAll('\\', '/').split('/').at(-1).match(/^(?:lib)?codeterm_service_core-([a-f0-9]{16})(?:\.|$)/)?.[1];
        if (!hash) continue;
        const unit = units.get(hash) ?? { hash, features: item.features, profile: item.profile, phases: [] };
        if (!unit.phases.includes(phase)) unit.phases.push(phase);
        units.set(hash, unit);
      }
    }
  }
  return [...units.values()].sort((a, b) => a.hash.localeCompare(b.hash));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const directory = process.argv[2];
  const logs = Object.fromEntries(fs.readdirSync(directory).filter(file => file.endsWith('.log')).map(file => [file, fs.readFileSync(path.join(directory, file), 'utf8')]));
  const units = coreUnits(logs);
  if (!units.length) throw new Error('No completed service-core artifacts in the cycle');
  const report = { sha: process.env.CT_BUILD_SOURCE_SHA, metadataHashes: units.length, featureSets: new Set(units.map(unit => JSON.stringify(unit.features))).size, units };
  fs.writeFileSync(path.join(directory, 'units.json'), JSON.stringify(report, null, 2));
  console.log(`feature-cycle: hashes=${report.metadataHashes} feature-sets=${report.featureSets} report=${path.join(directory, 'units.json')}`);
}
