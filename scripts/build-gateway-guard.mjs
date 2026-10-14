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

export function publicationGuard(directory) {
  const guard = path.join(directory, 'state.lock.guard');
  const identity = nativeIdentity(process.pid);
  if (identity.state !== 'present') throw new Error('Cannot establish publication helper identity');
  const ownerName = `owner.${randomUUID()}`;
  let prepared;
  const removeEmptyRetiredGuard = () => {
    const current = fs.statSync(guard);
    for (const name of fs.readdirSync(directory).filter(name => name.startsWith('state.guard.retired.'))) {
      const retired = path.join(directory, name);
      let record;
      try { record = JSON.parse(fs.readFileSync(path.join(retired, 'guard_identity.json'), 'utf8')); }
      catch { continue; }
      if (record.dev !== current.dev || record.ino !== current.ino) continue;
      const actual = nativeIdentity(record.pid);
      if (actual.state !== 'gone' && !(actual.state === 'present' && actual.token !== record.token)) continue;
      try { fs.rmdirSync(guard); fs.rmSync(retired, { recursive: true, force: true }); }
      catch (error) { if (!['ENOENT', 'ENOTEMPTY', 'EEXIST'].includes(error.code)) throw error; }
      return;
    }
  };
  const retireOwner = (name) => {
    // Retire only this unique owner; rmdir cannot remove a published successor.
    const retired = path.join(directory, `state.guard.retired.${name}`);
    try { fs.renameSync(path.join(guard, name), retired); }
    catch (error) { if (error.code === 'ENOENT') return; throw error; }
    try { fs.rmdirSync(guard); }
    catch (error) { if (!['ENOENT', 'ENOTEMPTY', 'EEXIST'].includes(error.code)) throw error; }
    if (!fs.existsSync(guard)) fs.rmSync(retired, { recursive: true, force: true });
  };
  return {
    acquire() {
      if (fs.existsSync(guard)) {
        let names;
        try { names = fs.readdirSync(guard); }
        catch (error) { if (['ENOENT', 'EACCES'].includes(error.code)) return false; throw error; }
        if (names.length === 0) { removeEmptyRetiredGuard(); return false; }
        if (names.length !== 1 || !/^owner\.[a-z0-9-]+$/.test(names[0])) return false;
        const owner = path.join(guard, names[0]);
        let pid, token;
        try {
          pid = fs.readFileSync(path.join(owner, 'pid'), 'utf8').trim();
          token = fs.readFileSync(path.join(owner, 'start_token'), 'utf8').trim();
        } catch (error) { if (error.code === 'ENOENT' || error.code === 'EACCES') return false; throw error; }
        if (!pid || !token) return false;
        const actual = nativeIdentity(pid);
        if (actual.state !== 'gone' && !(actual.state === 'present' && actual.token !== token)) return false;
        retireOwner(names[0]);
      }
      if (fs.existsSync(guard)) return false;
      prepared = fs.mkdtempSync(path.join(directory, 'state.guard.pending.'));
      const owner = path.join(prepared, ownerName);
      fs.mkdirSync(owner);
      fs.writeFileSync(path.join(owner, 'pid'), `${process.pid}\n`);
      fs.writeFileSync(path.join(owner, 'start_token'), `${identity.token}\n`);
      const inode = fs.statSync(prepared);
      fs.writeFileSync(path.join(owner, 'guard_identity.json'), JSON.stringify({ pid: process.pid, token: identity.token, dev: inode.dev, ino: inode.ino }));
      try { fs.renameSync(prepared, guard); prepared = undefined; return true; }
      catch (error) { if (!['EEXIST', 'ENOTEMPTY', 'EACCES', 'EPERM'].includes(error.code)) throw error; return false; }
      finally { if (prepared) { fs.rmSync(prepared, { recursive: true, force: true }); prepared = undefined; } }
    },
    release() {
      const released = path.join(directory, `state.guard.released.${ownerName}`);
      fs.renameSync(guard, released);
      fs.rmSync(released, { recursive: true, force: true });
    },
  };
}
