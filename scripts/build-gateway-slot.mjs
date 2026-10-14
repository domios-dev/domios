// Caller: scripts/build-gateway.sh
import fs from 'node:fs';
import path from 'node:path';

export function slotSnapshot(directory) {
  const read = file => {
    try { return fs.readFileSync(file, 'utf8').trim(); }
    catch (error) { if (['ENOENT', 'EACCES', 'EPERM'].includes(error.code)) return ''; throw error; }
  };
  const tickets = fs.readdirSync(path.join(directory, 'queue')).sort();
  return JSON.stringify([read(path.join(directory, 'lock', 'job_id')), tickets]);
}

export async function waitForSlotChange(directory, previous, deadline) {
  await new Promise((resolve, reject) => {
    let timer;
    const finish = error => {
      clearTimeout(timer);
      watcher.close();
      if (error) reject(error); else resolve();
    };
    const check = () => {
      try { if (slotSnapshot(directory) !== previous) finish(); }
      catch (error) { finish(error); }
    };
    // Subscribe before checking the captured state, including transitions between release and spawn.
    const watcher = fs.watch(directory, { recursive: true }, check);
    watcher.on('error', finish);
    timer = setTimeout(() => finish(new Error('Timed out waiting for build gateway slot.')), Math.max(0, deadline - Date.now()));
    check();
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
