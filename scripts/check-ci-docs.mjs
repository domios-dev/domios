import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export function documentReferenceErrors(documents) {
  const names = new Set(Object.keys(documents));
  const errors = [];
  for (const [name, text] of Object.entries(documents)) {
    const seen = new Set();
    for (const fragment of text.split(String.fromCharCode(96))) {
      const command = fragment.trim();
      if (!command.startsWith('domios docs ')) continue;
      const target = command.slice('domios docs '.length);
      if (['<NAME>', '--help'].includes(target)) continue;
      if (!names.has(target) || target === name || seen.has(target)) errors.push(name + ': invalid or repeated whole-document reference ' + target);
      seen.add(target);
    }
  }
  return errors;
}

export function staleMapSources(rows, readSource) {
  return [...new Set(rows.filter(row => row.source && createHash('sha256')
    .update(readSource(row.source.path).replaceAll('\r\n', '\n')).digest('hex') !== row.source.sha256)
    .map(row => row.source.path))];
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const registry = fs.readFileSync('cli/src/cmd/docs.rs', 'utf8');
  const entries = [...registry.matchAll(/doc!\(\s*"([^"]+)"\s*,\s*"(?:\\.|[^"\\])*"\s*,\s*"([^"]+)"\s*\)/g)];
  if (!entries.length) throw new Error('Embedded document registry is empty');
  const documents = Object.fromEntries(entries.map(([, name, file]) =>
    [name, fs.readFileSync(path.resolve('cli/src/cmd', file), 'utf8')]));
  const errors = documentReferenceErrors(documents);
  const rows = fs.readFileSync('essences/map.jsonl', 'utf8').trim().split('\n').map(line => JSON.parse(line));
  const stale = staleMapSources(rows, file => fs.readFileSync(file, 'utf8'));
  if (stale.length) errors.push('Stale essence map sources: ' + stale.join(', ') +
    '; regenerate with: cargo run --manifest-path cli/Cargo.toml --bin codeterm -- essence-map generate --root .');
  JSON.parse(fs.readFileSync('generated/cli/cli-manifest.json', 'utf8'));
  console.log('CLI/essence regeneration: bash build.sh cli-docs; cargo run --manifest-path cli/Cargo.toml --bin codeterm -- essence-map generate --root .');
  console.log('The fast CLI suite verifies manifest, command paths, CLI docs and essence-map freshness against compiled source.');
  if (errors.length) throw new Error(errors.join('\n'));
  console.log('Embedded document references and essence source digests are current.');
}
