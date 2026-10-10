export function clockBounds(samples) {
  if (!samples.length) throw new Error('Clock exchange samples required');
  let lower = -Infinity, upper = Infinity;
  for (const { sentMs, receivedMs, remoteMs } of samples) {
    if (![sentMs, receivedMs, remoteMs].every(Number.isFinite) || receivedMs < sentMs) throw new Error('Invalid clock exchange');
    lower = Math.max(lower, remoteMs - receivedMs);
    upper = Math.min(upper, remoteMs - sentMs);
  }
  if (lower > upper) throw new Error('Clock samples disagree; possible clock step');
  return { lower, upper };
}
export function correlate(home, view, offset, capMs = 1000) {
  if (!home.eventId || !home.tabId || !home.sessionId || home.evidenceKind !== 'rendered' || view.evidenceKind !== 'rendered') throw new Error('Correlated rendered evidence required on both views');
  if (home.eventId !== view.eventId || home.tabId !== view.tabId || home.sessionId !== view.sessionId || JSON.stringify(home.state) !== JSON.stringify(view.state)) throw new Error('Views disagree on event, identity or state');
  if (!Number.isFinite(capMs) || capMs < 0) throw new Error('Invalid parity cap');
  if (!Number.isFinite(home.atMs) || !Number.isFinite(view.atMs) || !Number.isFinite(offset.lower) || !Number.isFinite(offset.upper) || offset.lower > offset.upper) throw new Error('Invalid clock bounds');
  const lag = { lower: view.atMs - offset.upper - home.atMs, upper: view.atMs - offset.lower - home.atMs };
  if (Math.max(Math.abs(lag.lower), Math.abs(lag.upper)) > capMs) throw new Error(`Parity not proven within ${capMs}ms: [${lag.lower}, ${lag.upper}]`);
  return lag;
}
