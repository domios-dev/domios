// Caller: build.sh; scripts/tests/dev-instance-policy.test.mjs
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

export function validateDevInstance({ worktree, instance = '', port = '7686' }) {
  const selected = Number(port);
  if (!/^\d+$/.test(port) || !Number.isInteger(selected) || selected < 1 || selected > 65535) {
    throw new Error('CODETERM_API_PORT must be a valid nonzero TCP port');
  }
  if (selected === 7685) {
    throw new Error('development refuses production port 7685; set a private CODETERM_API_PORT');
  }
  if (worktree && !instance.trim()) {
    throw new Error('non-canonical worktree dev requires CODETERM_INSTANCE and a private CODETERM_API_PORT');
  }
  if (instance.trim() && selected === 7686) {
    throw new Error('a named dev instance requires its own CODETERM_API_PORT, distinct from 7685 and 7686');
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const gitPath = flag => execFileSync('git', ['rev-parse', '--path-format=absolute', flag], { encoding: 'utf8' }).trim();
    validateDevInstance({
      worktree: gitPath('--git-dir') !== gitPath('--git-common-dir'),
      instance: process.env.CODETERM_INSTANCE,
      port: process.env.CODETERM_API_PORT || '7686',
    });
  } catch (error) {
    console.error(`ERROR: ${error.message}`);
    process.exitCode = 2;
  }
}
