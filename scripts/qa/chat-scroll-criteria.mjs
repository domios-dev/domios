const paintedEdges = new Set([
  'paint',
  'inset-shrink-paint',
  'inset-restore-paint',
  'composer-expand-paint',
  'composer-clear-paint',
  'hydrated-reload-paint',
]);

export function isPaintedSample(sample) {
  return paintedEdges.has(sample.edge);
}

export function paintedGapSamples(samples, { afterSend = false } = {}) {
  const optimistic = afterSend ? samples.findIndex((sample) => sample.edge === 'optimistic') : -1;
  const start = optimistic >= 0 ? optimistic + 1
    : afterSend ? Math.max(0, samples.findIndex((sample) => sample.edge === 'before-send') + 1) : 0;
  return samples.slice(start).filter((sample) => isPaintedSample(sample) && sample.gap > 1);
}
