export class RemotePtyIdCache {
  private readonly values = new Map<string, string>();
  private readonly lookups = new Map<string, Promise<string>>();
  private readonly generations = new Map<string, number>();

  resolve(tabId: string, lookup: () => Promise<string>): Promise<string> {
    const cached = this.values.get(tabId);
    if (cached) return Promise.resolve(cached);

    const pending = this.lookups.get(tabId);
    if (pending) return pending;

    const generation = this.generations.get(tabId) ?? 0;
    const request = Promise.resolve()
      .then(lookup)
      .then((ptyId) => {
        if (!ptyId) throw new Error("Remote tab PTY unavailable");
        if ((this.generations.get(tabId) ?? 0) === generation) this.values.set(tabId, ptyId);
        return ptyId;
      })
      .finally(() => {
        if (this.lookups.get(tabId) === request) this.lookups.delete(tabId);
      });
    this.lookups.set(tabId, request);
    return request;
  }

  invalidate(tabId: string): void {
    this.generations.set(tabId, (this.generations.get(tabId) ?? 0) + 1);
    this.values.delete(tabId);
    this.lookups.delete(tabId);
  }
}

export interface RemotePtyWriteResponse<T> {
  ok?: boolean;
  status?: number;
  data?: T;
  error?: string | null;
}

export async function writeRemotePty<T>(
  cache: RemotePtyIdCache,
  tabId: string,
  resolve: () => Promise<string>,
  write: (ptyId: string) => Promise<RemotePtyWriteResponse<T>>,
): Promise<T | undefined> {
  const ptyId = await cache.resolve(tabId, resolve);
  const response = await write(ptyId);
  if (!response.ok) {
    if (response.status === 404) cache.invalidate(tabId);
    throw new Error(response.error ?? "Remote tab unavailable");
  }
  return response.data;
}
