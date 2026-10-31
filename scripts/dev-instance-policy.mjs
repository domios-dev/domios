// Caller: build.sh
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

export function validateDevInstance({ worktree, instance = '', port = '7686' }) {
  const selected = Number(port);
  if (!/^\d+$/.test(port) || !Number.isInteger(selected) || selected < 1 || selected > 65535) {
    throw new Error('CODETERM_DEV_API_PORT must be a valid nonzero TCP port');
  }
  if (selected === 7685) {
    throw new Error('development refuses production port 7685; set a private CODETERM_DEV_API_PORT');
  }
  if (worktree && !instance.trim()) {
    throw new Error('non-canonical worktree dev requires CODETERM_DEV_INSTANCE and a private CODETERM_DEV_API_PORT');
  }
  if (instance.trim() && selected === 7686) {
    throw new Error('a named dev instance requires its own CODETERM_DEV_API_PORT, distinct from 7685 and 7686');
  }
  if (/[\r\n]/.test(instance)) throw new Error('CODETERM_DEV_INSTANCE must be a single line');
}

export function devLaunchSelection(environment, worktree) {
  const selection = {
    instance: environment.CODETERM_DEV_INSTANCE ?? '',
    port: environment.CODETERM_DEV_API_PORT ?? '7686',
  };
  validateDevInstance({ worktree, ...selection });
  return selection;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const gitPath = flag => execFileSync('git', ['rev-parse', '--path-format=absolute', flag], { encoding: 'utf8' }).trim();
    const selection = devLaunchSelection(process.env, gitPath('--git-dir') !== gitPath('--git-common-dir'));
    console.log(`${selection.instance}:${selection.port}`);
  } catch (error) {
    console.error(`ERROR: ${error.message}`);
    process.exitCode = 2;
  }
}
