// Caller: scripts/cache-policy.sh, scripts/cargo-build-gc.mjs
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const normalize = value => {
  const result = path.resolve(value).replaceAll('\\', '/');
  return process.platform === 'win32' ? result.toLowerCase() : result;
};

export function intermediateDirectory(checkout, target, cache) {
  const root = fs.realpathSync(checkout);
  let ancestor = path.resolve(target);
  const missing = [];
  while (!fs.existsSync(ancestor)) {
    missing.unshift(path.basename(ancestor));
    const parent = path.dirname(ancestor);
    if (parent === ancestor) throw new Error(`target has no physical ancestor: ${target}`);
    ancestor = parent;
  }
  const final = path.join(fs.realpathSync(ancestor), ...missing);
  const hash = createHash('sha256').update(JSON.stringify([normalize(root), normalize(final)])).digest('hex').slice(0, 16);
  return { directory: path.join(cache, 'build', hash.slice(0, 2), hash.slice(2)), checkout: root, target: final };
}

export function ownedIntermediateMappings(root) {
  if (!fs.existsSync(root)) return [];
  const mappings = [];
  for (const bucket of fs.readdirSync(root).filter(name => /^[a-f0-9]{2}$/.test(name))) {
    for (const suffix of fs.readdirSync(path.join(root, bucket)).filter(name => /^[a-f0-9]{14}$/.test(name))) {
      const build = path.join(root, bucket, suffix);
      const marker = path.join(build, '.domios-intermediates.json');
      if (!fs.existsSync(marker)) continue;
      if (fs.lstatSync(marker).isSymbolicLink()) throw new Error(`unsafe intermediate marker: ${marker}`);
      const record = JSON.parse(fs.readFileSync(marker, 'utf8'));
      if (record.schema !== 1 || !path.isAbsolute(record.checkout) || !path.isAbsolute(record.target)) {
        throw new Error(`invalid intermediate marker: ${marker}`);
      }
      if (fs.existsSync(record.checkout)) mappings.push({ checkout: record.checkout, build });
    }
  }
  return mappings;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [checkout, target, cache = path.join(os.homedir(), '.cargo')] = process.argv.slice(2);
  const record = intermediateDirectory(checkout, target, cache);
  fs.mkdirSync(record.directory, { recursive: true });
  const marker = path.join(record.directory, '.domios-intermediates.json');
  if (fs.existsSync(marker) && fs.lstatSync(marker).isSymbolicLink()) throw new Error(`unsafe intermediate marker: ${marker}`);
  fs.writeFileSync(marker, JSON.stringify({ schema: 1, checkout: record.checkout, target: record.target }));
  console.log(record.directory.replaceAll('\\', '/'));
}
