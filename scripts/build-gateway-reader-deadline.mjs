// Caller: build.sh gateway-fake|gateway-sleep|gateway-status|gateway-cancel|gateway-policy-probe|gateway-log-probe
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { pathToFileURL } from 'node:url';
import fs from 'node:fs';
import path from 'node:path';

export function watchReaderProgress(file, milliseconds, expire, io = {}) {
  const schedule = io.schedule ?? setTimeout;
  const cancel = io.cancel ?? clearTimeout;
  const size = io.size ?? (() => { try { return fs.statSync(file).size; } catch { return 0; } });
  const watch = io.watch ?? (callback => fs.watch(path.dirname(file), callback));
  let previous = size();
  let watcher;
  let timer;
  const arm = () => { cancel(timer); timer = schedule(() => { watcher?.close(); expire(); }, milliseconds); };
  if (file) {
    try {
      watcher = watch(() => {
        const current = size();
        if (current > previous) { previous = current; arm(); }
      });
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
  arm();
  return () => { cancel(timer); watcher?.close(); };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [pid, token, deadline, log] = process.argv.slice(2);
  const script = fileURLToPath(new URL('./build-gateway.sh', import.meta.url)).replaceAll('\\', '/');
  watchReaderProgress(log, Number(deadline), () => {
    const result = spawnSync('bash', [
      '-c', 'source "$1"; ct_gateway_identity_alive "$2" "$3" || exit 0; echo "ERROR: build gateway log reader did not reach EOF before its failure deadline" >&2; kill -KILL "$2"',
      'gateway', script, pid, token,
    ], { stdio: 'inherit' });
    process.exitCode = result.status ?? 74;
  });
}
