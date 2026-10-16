// Caller: scripts/lane-check.sh; Windows workspace wrapper with native command-line forwarding.
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { laneBuildDirectory } from './lane-build-dir.mjs';

export function laneNativeWrapper() {
  const source = realpathSync.native(path.join(import.meta.dirname, 'lane-rustc-wrapper.c'));
  const hash = createHash('sha256').update(source.toLowerCase()).update(readFileSync(source)).update(process.arch).digest('hex');
  const directory = path.join(laneBuildDirectory(), '..', 'codeterm-lane-wrappers');
  const executable = path.join(directory, `${hash}.exe`);
  if (existsSync(executable)) return executable;
  mkdirSync(directory, { recursive: true });
  const temporary = mkdtempSync(path.join(directory, 'compile-'));
  try {
    const result = spawnSync(process.env.CC || 'cl.exe', [
      '/nologo', '/O2', '/GS-', source, '/Fewrapper.exe', '/Fowrapper.obj',
      '/link', '/nodefaultlib', '/entry:mainCRTStartup', '/subsystem:console', 'kernel32.lib',
    ], { cwd: temporary, encoding: 'utf8', timeout: 60_000 });
    if (result.status !== 0) throw new Error(`Native lane wrapper compilation failed: ${result.error?.message ?? ''}\n${result.stdout}\n${result.stderr}`);
    try {
      renameSync(path.join(temporary, 'wrapper.exe'), executable);
    } catch (error) {
      if (!existsSync(executable)) throw error;
    }
    return executable;
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  console.log(laneNativeWrapper());
}
