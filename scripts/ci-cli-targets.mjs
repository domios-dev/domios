import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export function fastCliTargets(metadata) {
  const cli = metadata.packages.find(pkg => pkg.name === 'codeterm-cli');
  if (!cli) throw new Error('Missing CLI package');
  const targets = cli.targets.flatMap(target => {
    if (target.kind.includes('bin')) return ['--bin', target.name];
    if (target.kind.includes('test')) return ['--test', target.name];
    return [];
  });
  if (!targets.length) throw new Error('No fast CLI targets');
  return targets;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--locked', '--format-version', '1', '--manifest-path', 'cli/Cargo.toml'], { encoding: 'utf8' }));
  process.stdout.write(fastCliTargets(metadata).join('\n') + '\n');
}
