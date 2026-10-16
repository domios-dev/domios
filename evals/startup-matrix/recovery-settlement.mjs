export function processExit(child, signal) {
  return new Promise((resolve, reject) => {
    const cleanup = () => { child.removeListener('exit', exited); child.removeListener('error', failed); signal.removeEventListener('abort', aborted); };
    const exited = (code, terminatedBy) => {
      cleanup();
      if (code === 0) resolve();
      else reject(Error(`Owned exit helper failed: code=${code} signal=${terminatedBy}`));
    };
    const failed = error => { cleanup(); reject(error); };
    const aborted = () => { child.kill(); failed(signal.reason); };
    child.once('exit', exited);
    child.once('error', failed);
    signal.addEventListener('abort', aborted, { once: true });
    if (signal.aborted) aborted();
  });
}

export async function settleRecovery({ exit, recovered, readFinal }) {
  await Promise.all([exit, recovered]);
  return readFinal();
}
