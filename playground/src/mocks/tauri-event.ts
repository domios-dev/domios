import { mockListen } from "./backend";

export type UnlistenFn = () => void;

export const listen = mockListen;

export async function once(event: string, handler: Parameters<typeof mockListen>[1]): Promise<UnlistenFn> {
  return mockListen(event, handler);
}

export async function emit(): Promise<void> {}
