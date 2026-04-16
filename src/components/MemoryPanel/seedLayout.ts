export function hashString(s: string): number {
  let h = 2166136261 >>> 0;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

export function seedPosition(id: string, count: number): { x: number; y: number } {
  const h = hashString(id);
  const angle = (h / 0xffffffff) * Math.PI * 2;
  const ringRadius = Math.max(200, Math.sqrt(Math.max(1, count)) * 80);
  const jitter = ((h >>> 16) / 0xffff - 0.5) * ringRadius * 0.4;
  const r = ringRadius + jitter;
  return {
    x: Math.cos(angle) * r,
    y: Math.sin(angle) * r,
  };
}
