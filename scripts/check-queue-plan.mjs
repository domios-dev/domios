// Caller: build.sh check-queue; scripts/check-queue-plan.test.mjs
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export function checkQueuePlan(metadata, targets, changes, root) {
  const members = new Set(metadata.workspace_members);
  const packages = metadata.packages.filter(pkg => members.has(pkg.id));
  const requested = targets === 'all' ? packages.map(pkg => pkg.name) : targets.split('\n').filter(line => line.startsWith('app ')).map(line => line.slice(4));
  return packages.filter(pkg => requested.includes(pkg.name)).map(pkg => {
    const directory = path.relative(root, path.dirname(pkg.manifest_path)).replaceAll('\\', '/');
    const files = changes.split('\n').filter(Boolean).map(line => line.split('\t')).filter(fields => fields.slice(1).some(file => file.startsWith(`${directory}/`)));
    const tests = pkg.name !== 'codeterm-service-core' && files.some(([status, ...paths]) =>
      paths.some(file => /(?:^|\/)(?:tests\/|tests\.rs$|[^/]+_tests\.rs$)/.test(file)) ||
      (/^[AR]/.test(status) && paths.some(file => file.endsWith('/Cargo.toml'))));
    return { name: pkg.name, tests, bins: pkg.name === 'codeterm', lib: pkg.targets.some(target => target.kind.some(kind => ['lib', 'rlib', 'proc-macro', 'cdylib', 'staticlib'].includes(kind))) };
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [metadata, targets, changes] = process.argv.slice(2);
  for (const pkg of checkQueuePlan(JSON.parse(fs.readFileSync(metadata, 'utf8')), fs.readFileSync(targets, 'utf8').trim(), fs.readFileSync(changes, 'utf8'), process.cwd())) {
    console.log(`${pkg.name}\t${Number(pkg.tests)}\t${Number(pkg.bins)}\t${Number(pkg.lib)}`);
  }
}
