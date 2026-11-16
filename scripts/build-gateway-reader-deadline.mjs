// Caller: build.sh gateway-fake|gateway-sleep|gateway-status|gateway-cancel|gateway-policy-probe
import {bashInterpreter} from './bash-interpreter.mjs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { pathToFileURL } from 'node:url';

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [pid, token, deadline] = process.argv.slice(2);
  const script = fileURLToPath(new URL('./build-gateway.sh', import.meta.url)).replaceAll('\\', '/');
  setTimeout(() => {
    const result = spawnSync(bashInterpreter(), [
      '-c', 'source "$1"; ct_gateway_identity_alive "$2" "$3" || exit 0; echo "ERROR: build gateway log reader did not reach EOF before its failure deadline" >&2; kill -KILL "$2"',
      'gateway', script, pid, token,
    ], { stdio: 'inherit' });
    process.exitCode = result.status ?? 74;
  }, Number(deadline));
}
