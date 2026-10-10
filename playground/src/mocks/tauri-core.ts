import { mockInvoke } from "./backend";

export const invoke = mockInvoke;

export function isTauri(): boolean {
  return true;
}

export function transformCallback(): number {
  return 0;
}

export function convertFileSrc(path: string): string {
  return path;
}

export class Channel<T> {
  onmessage: (message: T) => void = () => undefined;
}
