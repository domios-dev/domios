export function summarizeAllocations(profile) {
  const own = new Map(), inclusive = new Map();
  const key = frame => `${frame.functionName || '(anonymous)'} ${frame.url || '(native)'}:${frame.lineNumber + 1}:${frame.columnNumber + 1}`;
  const add = (map, name, bytes) => map.set(name, (map.get(name) ?? 0) + bytes);
  function walk(node, ancestors) {
    const name = key(node.callFrame), stack = new Set([...ancestors, name]);
    add(own, name, node.selfSize);
    for (const caller of stack) add(inclusive, caller, node.selfSize);
    for (const child of node.children ?? []) walk(child, stack);
  }
  walk(profile.head, new Set());
  const ranked = map => [...map].map(([frame, bytes]) => ({frame, bytes})).sort((a,b) => b.bytes-a.bytes).slice(0,50);
  return {
    estimatedAllocatedBytes: [...own.values()].reduce((sum, value) => sum + value, 0),
    sampleCount: profile.samples?.length ?? 0,
    self: ranked(own), inclusive: ranked(inclusive),
    interpretation: 'Sampled allocation volume including collected objects; not retained heap or process private bytes. Inclusive rows overlap.',
  };
}
