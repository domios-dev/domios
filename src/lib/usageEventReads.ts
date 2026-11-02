const eventReads = new WeakMap<object, Map<string, Promise<unknown>>>();

export function readUsageOnEvent<T>(
  event: object | undefined,
  command: string,
  args: Record<string, unknown>,
  read: (command: string, args: Record<string, unknown>) => Promise<T>,
): Promise<T> {
  if (!event) return read(command, args);
  let reads = eventReads.get(event);
  if (!reads) {
    reads = new Map();
    eventReads.set(event, reads);
  }
  const key = JSON.stringify([command, args]);
  const existing = reads.get(key);
  if (existing) return existing as Promise<T>;
  const result = read(command, args);
  reads.set(key, result);
  return result;
}
