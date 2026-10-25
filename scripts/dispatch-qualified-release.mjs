import { sourceRepository } from './source-repository.mjs';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { requireQualification } from './ci-qualification.mjs';

const gh = args => execFileSync('gh', args, { encoding: 'utf8', timeout: 120000 }).trim();

export function dispatchQualifiedRelease(sha, branch, {
  repository = sourceRepository(),
  qualify = requireQualification, run = gh,
} = {}) {
  if (!/^[a-f0-9]{40}$/.test(sha) || !/^release\/\d+\.\d+\.\d+$/.test(branch)) {
    throw new Error('Usage: dispatch-qualified-release.mjs SHA release/X.Y.Z');
  }
  if (run(['api', 'user', '--jq', '.login']) !== 'rollacode') throw new Error('Owner dispatch requires rollacode');
  const head = () => run(['api', 'repos/' + repository + '/git/ref/heads/' + branch, '--jq', '.object.sha']);
  if (head() !== sha) throw new Error('Release branch no longer points at reviewed SHA');
  qualify(sha, { repository });
  if (head() !== sha) throw new Error('Release branch moved during qualification readback');
  run(['workflow', 'run', 'release.yml', '--repo', repository, '--ref', branch,
    '-f', 'tag=' + sha, '-f', 'dry_run=true', '-f', 'mirror=false']);
  return sha;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  console.log('Dispatched qualified dry-run source ' + dispatchQualifiedRelease(...process.argv.slice(2)));
}
