// Caller: scripts/build-gateway.sh
import fs from 'node:fs';
import path from 'node:path';

export function slotSnapshot(directory) {
  const read = file => {
    try { return fs.readFileSync(file, 'utf8').trim(); }
    catch (error) { if (['ENOENT', 'EACCES', 'EPERM'].includes(error.code)) return ''; throw error; }
  };
  const tickets = fs.readdirSync(path.join(directory, 'queue')).sort();
  const owner = ['job_id', 'pid', 'start_token', 'pid_namespace', 'native_pid', 'native_start_token']
    .map(name => read(path.join(directory, 'lock', name)));
  return JSON.stringify([owner, tickets]);
}

export async function waitForSlotChange(directory, previous, deadline, { watch = fs.watch } = {}) {
  await new Promise((resolve, reject) => {
    let timer, lockWatcher, finished = false;
    const watchers = [];
    const finish = error => {
      if (finished) return;
      finished = true;
      clearTimeout(timer);
      lockWatcher?.close();
      for (const watcher of watchers) watcher.close();
      if (error) reject(error); else resolve();
    };
    const check = () => {
      if (finished) return;
      try {
        lockWatcher?.close();
        lockWatcher = undefined;
        try {
          lockWatcher = watch(path.join(directory, 'lock'), check);
          lockWatcher.on('error', error => ['ENOENT', 'EPERM'].includes(error.code) ? check() : finish(error));
        } catch (error) {
          if (error.code !== 'ENOENT' && !(error.code === 'EPERM' && !fs.existsSync(path.join(directory, 'lock')))) throw error;
        }
        if (slotSnapshot(directory) !== previous) finish();
      }
      catch (error) { finish(error); }
    };
    try {
      // Artifact trees and transient mutex directories are not slot state.
      for (const stable of [directory, path.join(directory, 'queue')]) {
        const watcher = watch(stable, check);
        watchers.push(watcher);
        watcher.on('error', finish);
      }
      timer = setTimeout(() => finish(new Error('Timed out waiting for build gateway slot.')), Math.max(0, deadline - Date.now()));
      check();
    } catch (error) { finish(error); }
  });
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(import.meta.filename)) {
  const [operation, directory, previous, deadline] = process.argv.slice(2);
  try {
    if (operation === 'snapshot') process.stdout.write(slotSnapshot(directory));
    else if (operation === 'wait') await waitForSlotChange(directory, previous, Number(deadline));
    else throw new Error('Expected snapshot or wait');
  } catch (error) { console.error(error.message); process.exitCode = 75; }
}
