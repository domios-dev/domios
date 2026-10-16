export type Callback = (...args: never[]) => unknown;

export function createLatestCallback<T extends Callback>(slot: { current: T }): T {
  return ((...args: Parameters<T>) => slot.current(...args)) as T;
}
