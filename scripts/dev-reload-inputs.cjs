const { createHash } = require('node:crypto');
const { existsSync, readFileSync } = require('node:fs');
const path = require('node:path');

const fullDev = 'Client/build inputs changed; run ./build.sh dev instead.';
const serverToolInputs = new Set(['build.sh', 'scripts/dev-server.mjs', 'scripts/dev-server-bootstrap.ps1',
  'scripts/dev-reload-inputs.cjs', 'scripts/server-artifact.mjs']);

// A client or shared crate is linked into the native window, so editing one needs a client rebuild.
const layers = JSON.parse(readFileSync(path.join(__dirname, 'crate-layers.json'), 'utf8'));
const clientLinked = new Set([...layers.client, ...layers.shared]);

function changeKind(file) {
  if (serverToolInputs.has(file)) return 'server';
  if (file === 'scripts/crate-layers.json') return 'client';
  const crate = /^src-tauri\/crates\/([^/]+)\//.exec(file)?.[1];
  if (crate && clientLinked.has(crate)) return 'client';
  if (/^(src\/|public\/)/.test(file)) return 'frontend';
  if (/^(src-tauri\/crates\/|cli\/|crates\/|plugins\/|skills\/)/.test(file)) return 'server';
  return 'client';
}

function reloadPlan(previous, current, cliRoots) {
  if (!previous || typeof previous !== 'object') throw Error(fullDev);
  const changed = [...new Set([...Object.keys(previous), ...Object.keys(current)])]
    .filter(file => previous[file] !== current[file]);
  if (changed.some(file => changeKind(file) === 'client')) throw Error(fullDev);
  return { changed, cli: changed.some(file => cliRoots.some(root => file.startsWith(`${root}/`))) };
}

function inputSnapshot(repo, files) {
  return Object.fromEntries(files.map(file => [file, existsSync(path.join(repo, file))
    ? createHash('sha256').update(readFileSync(path.join(repo, file))).digest('hex') : null]));
}

function cliSourceRoots(repo) {
  const visited = new Set();
  function visit(directory) {
    const root = path.relative(repo, directory).replaceAll('\\', '/');
    if (visited.has(root)) return;
    if (root.startsWith('../') || path.isAbsolute(root)) throw Error('CLI source dependency is outside this checkout');
    visited.add(root);
    const manifest = path.join(directory, 'Cargo.toml');
    if (!existsSync(manifest)) return;
    for (const match of readFileSync(manifest, 'utf8').matchAll(/\bpath\s*=\s*"([^"\n]+)"/g)) {
      const dependency = path.resolve(directory, match[1]);
      if (existsSync(path.join(dependency, 'Cargo.toml'))) visit(dependency);
    }
  }
  visit(path.join(repo, 'cli'));
  return [...visited].sort();
}

module.exports = { changeKind, reloadPlan, inputSnapshot, cliSourceRoots };
