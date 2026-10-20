function plainRecord(value: object): boolean {
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

/** Structural equality for JSON-shaped values; anything else, such as a Map or a function, is equal only to itself. */
export function sameJson(left: unknown, right: unknown): boolean {
  if (left === right) return true;
  if (typeof left !== "object" || typeof right !== "object" || left === null || right === null) return false;
  if (Array.isArray(left)) {
    if (!Array.isArray(right) || left.length !== right.length) return false;
    for (let index = 0; index < left.length; index++) if (!sameJson(left[index], right[index])) return false;
    return true;
  }
  if (Array.isArray(right) || !plainRecord(left) || !plainRecord(right)) return false;
  const leftRecord = left as Record<string, unknown>;
  const rightRecord = right as Record<string, unknown>;
  const keys = Object.keys(leftRecord);
  if (keys.length !== Object.keys(rightRecord).length) return false;
  for (const key of keys) {
    if (!(key in rightRecord) || !sameJson(leftRecord[key], rightRecord[key])) return false;
  }
  return true;
}

/** Keeps every previous entry whose content did not change: the previous record when none did, the incoming one when all did. */
export function shareRecord<T>(previous: Readonly<Record<string, T>>, next: Readonly<Record<string, T>>): Readonly<Record<string, T>> {
  if (previous === next) return next;
  let changed = Object.keys(previous).length !== Object.keys(next).length;
  let reused = 0;
  const shared: Record<string, T> = {};
  for (const [key, value] of Object.entries(next)) {
    const before = previous[key];
    if (key in previous && sameJson(before, value)) {
      shared[key] = before;
      reused++;
    } else {
      shared[key] = value;
      changed = true;
    }
  }
  if (!changed) return previous;
  return reused === 0 ? next : shared;
}
