export function parseBootPhase(line) {
  const match = line.match(/\[boot-phase\] tab=(\S+) pty=(\S+) phase=("[^"]+"|\S+) at_us=(\d+)/);
  if (match) return { tab: match[1], pty: match[2], phase: match[3].replaceAll('"', ''), atUs: Number(match[4]), line };
  const bound = line.match(/\[boot-request\] tab=(\S+) .* bound_at_us=(\d+)/);
  return bound ? { tab: bound[1], pty: '-', phase: 'request_bound', atUs: Number(bound[2]), line } : null;
}

export function phasesForTab(marks, tab) {
  const ptys = new Set(marks.filter(mark => mark.tab === tab && mark.pty !== '-').map(mark => mark.pty));
  return marks.filter(mark => mark.tab === tab || mark.tab === '-' && ptys.has(mark.pty));
}

export function captureBounded({ signal, collect, persist, close = () => {} }) {
  return new Promise((resolve, reject) => {
    let finished = false;
    const finish = outcome => {
      if (finished) return;
      finished = true;
      signal.removeEventListener('abort', aborted);
      try {
        persist(outcome);
        resolve(outcome);
      } catch (error) {
        reject(error);
      } finally {
        close();
      }
    };
    const aborted = () => finish({ status: 'incomplete', error: String(signal.reason) });
    signal.addEventListener('abort', aborted, { once: true });
    if (signal.aborted) return aborted();
    Promise.resolve().then(() => {
      if (!finished) return collect();
    }).then(result => finish({ status: 'completed', result }),
      error => finish({ status: 'incomplete', error: String(error) }));
  });
}
