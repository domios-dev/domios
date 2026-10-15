// Caller: scripts/lane-check.sh, scripts/cargo-build-gc.mjs; tests scripts/tests/lane-check.test.mjs
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export function laneBuildDirectory({ env = process.env, home = os.homedir(), platform = process.platform } = {}) {
  const paths = platform === 'win32' ? path.win32 : path.posix;
  const root = env.CODETERM_RUST_CACHE_ROOT;
  let base = root || env.CARGO_HOME || paths.join(home, '.cargo');
  if (platform === 'win32') base = base.replace(/^\/([a-zA-Z])(?=\/|$)/, '$1:');
  const directory = paths.resolve(base, root ? 'lanes' : 'codeterm-lanes');
  return platform === 'win32' ? directory.replaceAll('\\', '/') : directory;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  console.log(laneBuildDirectory());
}
