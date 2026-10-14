// Caller: scripts build-gateway-mutex.mjs, build-gateway-mutex.test.mjs
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';

export function nativeIdentity(pid) {
  if (!/^\d+$/.test(String(pid))) return { state: 'unknown' };
  if (process.platform === 'linux') {
    try {
      const stat = fs.readFileSync(`/proc/${pid}/stat`, 'utf8');
      const token = stat.slice(stat.lastIndexOf(') ') + 2).split(' ')[19];
      return token ? { state: 'present', token } : { state: 'unknown' };
    } catch (error) { return { state: error.code === 'ENOENT' ? 'gone' : 'unknown' }; }
  }
  const result = process.platform === 'win32'
    ? spawnSync('powershell.exe', ['-NoProfile', '-Command', `try { $p=[System.Diagnostics.Process]::GetProcessById(${pid}); [Console]::Write($p.StartTime.ToUniversalTime().Ticks) } catch [System.ArgumentException] { [Console]::Write('GONE') } catch { exit 2 }`], { encoding: 'utf8' })
    : spawnSync('ps', ['-p', String(pid), '-o', 'lstart='], { encoding: 'utf8' });
  if (result.error || result.signal) return { state: 'unknown' };
  const token = result.stdout?.trim();
  if (process.platform === 'win32' && result.status === 0 && token === 'GONE') return { state: 'gone' };
  if (process.platform !== 'win32' && result.status === 1 && !token) return { state: 'gone' };
  return result.status === 0 && token ? { state: 'present', token } : { state: 'unknown' };
}

export function publicationGuard(directory, identify = nativeIdentity) {
  const guard = path.join(directory, 'state.lock.guard');
  const reclamation = path.join(directory, 'state.guard.reclaim');
  let record;
  let identityUnavailable = false;
  const read = () => {
    try { return fs.readFileSync(guard, 'utf8'); }
    catch (error) { if (['ENOENT', 'EISDIR', 'EACCES', 'EPERM'].includes(error.code)) return ''; throw error; }
  };
  const create = () => {
    let fd;
    try {
      fd = fs.openSync(guard, 'wx');
      fs.writeFileSync(fd, record);
      return true;
    } catch (error) {
      if (['EEXIST', 'EACCES', 'EPERM'].includes(error.code)) return false;
      throw error;
    } finally { if (fd !== undefined) fs.closeSync(fd); }
  };
  return {
    get identityUnavailable() { return identityUnavailable; },
    acquire() {
      if (!record) {
        const identity = identify(process.pid);
        identityUnavailable = identity.state !== 'present';
        if (identityUnavailable) return false;
        record = JSON.stringify({ pid: process.pid, token: identity.token, owner: randomUUID() });
      }
      if (create()) return true;
      const observed = read();
      let owner;
      try { owner = JSON.parse(observed); } catch { return false; }
      if (!owner.pid || !owner.token) return false;
      const actual = nativeIdentity(owner.pid);
      if (actual.state !== 'gone' && !(actual.state === 'present' && actual.token !== owner.token)) return false;
      let reaper;
      try { reaper = fs.openSync(reclamation, 'wx'); }
      catch (error) { if (['EEXIST', 'EACCES', 'EPERM'].includes(error.code)) return false; throw error; }
      try {
        // Only one reaper can unlink the observed dead owner; a successor never matches its record.
        if (read() !== observed) return false;
        fs.unlinkSync(guard);
        return create();
      } finally { fs.closeSync(reaper); fs.unlinkSync(reclamation); }
    },
    release() {
      if (read() !== record) throw new Error('Publication guard ownership changed before release');
      fs.unlinkSync(guard);
    },
  };
}
