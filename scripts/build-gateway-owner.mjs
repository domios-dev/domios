// Caller: scripts/build-gateway.sh; scripts/build-gateway-owner.test.mjs
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { nativeIdentity } from './build-gateway-guard.mjs';

export function ownerState(owner, identify = nativeIdentity, platform = process.platform) {
  const pid = platform === 'win32' ? owner.native_pid : owner.pid;
  const expected = platform === 'win32' ? owner.native_start_token : owner.start_token;
  if (!/^\d+$/.test(String(pid ?? '')) || !expected) return 'unknown';
  const observed = identify(pid);
  if (observed.state === 'gone') return 'gone';
  if (observed.state !== 'present' || !observed.token) return 'unknown';
  return observed.token === expected ? 'alive' : 'gone';
}

export function readOwner(directory) {
  const read = name => { try { return fs.readFileSync(path.join(directory, name), 'utf8').trim(); } catch { return ''; } };
  return Object.fromEntries(['pid', 'start_token', 'pid_namespace', 'native_pid', 'native_start_token'].map(name => [name, read(name)]));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [operation, value] = process.argv.slice(2);
  if (operation === 'token') {
    const identity = nativeIdentity(value);
    if (identity.state === 'present' && identity.token) process.stdout.write(identity.token);
    else process.exitCode = 2;
  } else if (operation === 'state') process.stdout.write(ownerState(readOwner(value)));
  else throw new Error('Expected owner token or state');
}
