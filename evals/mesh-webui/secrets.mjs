import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {fileURLToPath} from 'node:url';
const exec = promisify(execFile);
const wrapper = fileURLToPath(new URL('./dc.sh',import.meta.url));
export async function gatewayToken(machine) {
  let stdout;
  try {
    ({stdout} = await exec('bash',[wrapper,'mem','secret','get','--name',machine.tokenSecret],
      {timeout:30_000,maxBuffer:64*1024}));
  } catch {
    throw new Error('Gateway secret lookup failed; no credentials logged');
  }
  const token = stdout.trim();
  if (!token) throw new Error('Gateway secret is empty');
  return token;
}
