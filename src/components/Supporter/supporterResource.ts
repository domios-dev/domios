import { listen } from "../../lib/bridge";
import type { ApiConfig } from "../../lib/types";
import { daemonFetch } from "../../lib/webApi";
import { authHeaders } from "../MemoryPanel/memoryApi";

export const SUPPORTER_JOURNAL_EVENT = "supporter-journal-appended";
interface Snapshot {
  data: unknown;
  failed: boolean;
}
const EMPTY: Snapshot = { data: null, failed: false };
const stores = new Map<string, SupporterResource>();

class SupporterResource {
  private snapshot: Snapshot = EMPTY;
  private subscribers = new Set<() => void>();
  private generation = 0;
  private controller?: AbortController;
  private unlisten?: () => void;
  private reading = false;
  private dirty = false;

  constructor(
    private api: ApiConfig | undefined,
    private path: string,
    private key: string,
  ) {}

  getSnapshot = () => this.snapshot;
  subscribe = (callback: () => void) => {
    this.subscribers.add(callback);
    if (this.subscribers.size === 1 && this.api?.enabled) this.start();
    return () => {
      this.subscribers.delete(callback);
      if (this.subscribers.size !== 0) return;
      ++this.generation;
      this.controller?.abort();
      this.unlisten?.();
      this.unlisten = undefined;
      this.reading = false;
      this.dirty = false;
      stores.delete(this.key);
    };
  };

  private publish(snapshot: Snapshot) {
    this.snapshot = snapshot;
    for (const subscriber of this.subscribers) subscriber();
  }

  private start() {
    const generation = ++this.generation;
    stores.set(this.key, this);
    listen(SUPPORTER_JOURNAL_EVENT, () => { void this.refresh(); })
      .then((off) => {
        if (generation !== this.generation) {
          off();
          return;
        }
        this.unlisten = off;
        void this.refresh();
      })
      .catch(() => {
        if (generation === this.generation) this.publish({ data: null, failed: true });
      });
  }

  private async refresh() {
    if (!this.api || this.subscribers.size === 0) return;
    if (this.reading) {
      this.dirty = true;
      return;
    }
    this.reading = true;
    this.dirty = false;
    const generation = this.generation;
    this.controller = new AbortController();
    try {
      const response = await daemonFetch(this.api, this.path, {
        headers: authHeaders(this.api),
        signal: this.controller.signal,
      });
      if (!response.ok) throw new Error("Supporter unavailable");
      const data: unknown = await response.json();
      if (generation === this.generation && !this.dirty) this.publish({ data, failed: false });
    } catch {
      if (generation === this.generation && !this.dirty) this.publish({ ...this.snapshot, failed: true });
    } finally {
      if (generation === this.generation) {
        this.reading = false;
        if (this.dirty) void this.refresh();
      }
    }
  }
}

export function supporterResource(api: ApiConfig | undefined, path: string) {
  const key = JSON.stringify([api?.enabled, api?.host, api?.port, api?.token, path]);
  const current = stores.get(key);
  if (current) return current;
  const store = new SupporterResource(api, path, key);
  stores.set(key, store);
  return store;
}
