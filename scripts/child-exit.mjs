// Caller: scripts collect-rust-warnings.mjs, ci-rust-tests.mjs
// Settles a piped CI step on its own exit: a descendant that inherited the pipes (a lazily started sccache server)
// must not hold the step open, so output is drained only up to a failure bound and then released.
export const OUTPUT_DRAIN_MS = 30_000;

const ended = stream => new Promise(resolve => {
  if (stream.readableEnded || stream.destroyed) return resolve();
  stream.once('end', resolve);
  stream.once('close', resolve);
});

export function childExit(child, { drainMs = OUTPUT_DRAIN_MS, warn = message => process.stderr.write(message + '\n') } = {}) {
  const streams = [child.stdout, child.stderr].filter(Boolean);
  return new Promise(resolve => {
    let settled = false, error;
    child.on('error', cause => {
      error ??= cause;
      // A child that never spawned emits no exit.
      if (child.pid !== undefined || settled) return;
      settled = true;
      for (const stream of streams) stream.destroy();
      resolve({ code: null, signal: null, error, held: false });
    });
    child.once('exit', async (code, signal) => {
      if (settled) return;
      settled = true;
      let timer;
      const held = await Promise.race([
        Promise.all(streams.map(ended)).then(() => false),
        new Promise(done => { timer = setTimeout(() => done(true), drainMs); }),
      ]);
      clearTimeout(timer);
      if (held) {
        for (const stream of streams) stream.destroy();
        warn(`::warning::${child.spawnfile} exited (${signal ?? code}) but a descendant still held its output ${drainMs / 1000}s later; stopped reading it`);
      }
      resolve({ code, signal, error, held });
    });
  });
}
