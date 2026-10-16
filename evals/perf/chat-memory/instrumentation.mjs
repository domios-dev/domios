export function memoryInstrumentation(flags) {
  const mode = flags.instrumentation ?? 'full';
  if (!['full','minimal'].includes(mode)) throw Error('Instrumentation must be full or minimal');
  if (mode === 'minimal' && (flags.snapshots === 'yes' || flags.allocations === 'yes')) {
    throw Error('Minimal instrumentation cannot collect snapshots or allocation profiles');
  }
  return {mode, profiler:mode === 'full'};
}
