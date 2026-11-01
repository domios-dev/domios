interface SnapshotReadinessPorts {
  readyVersion(): number;
  subscribeReady(listener: () => void): () => void;
  refresh(): void;
}

export function mountSnapshotReadiness(ports: SnapshotReadinessPorts): () => void {
  let active = true;
  let observed = ports.readyVersion();
  const stop = ports.subscribeReady(() => {
    if (!active) return;
    const version = ports.readyVersion();
    if (version <= observed) return;
    observed = version;
    ports.refresh();
  });
  ports.refresh();
  return () => { active = false; stop(); };
}
