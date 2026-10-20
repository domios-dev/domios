import { scopedAutomationGraph } from "./model";
import { DEV_BUILD } from "../../lib/devBuild";
import { sameJson } from "../../lib/sameJson";
import { automationKey, type AutomationAttachment, type AutomationGraph, type AutomationGraphAdapter, type AutomationIdentity } from "./types";

export interface AutomationSnapshot {
  graph: AutomationGraph | null;
  loading: boolean;
  failed: boolean;
  stopping: string | null;
  stopFailed: string | null;
}

export class AutomationResource {
  private snapshot: AutomationSnapshot = { graph: null, loading: true, failed: false, stopping: null, stopFailed: null };
  private listeners = new Set<() => void>();
  private generation = 0;
  private unlisten?: () => void;
  private controller?: AbortController;
  private reading = false;
  private dirty = false;

  constructor(private adapter: AutomationGraphAdapter, private attachment: AutomationAttachment) {}

  getSnapshot = () => this.snapshot;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    if (this.listeners.size === 1) this.start();
    return () => {
      this.listeners.delete(listener);
      if (this.listeners.size) return;
      ++this.generation;
      this.controller?.abort();
      this.unlisten?.();
      this.unlisten = undefined;
      this.reading = false;
      this.dirty = false;
      this.snapshot = { graph: null, loading: true, failed: false, stopping: null, stopFailed: null };
    };
  };

  private publish(change: Partial<AutomationSnapshot>) {
    this.snapshot = { ...this.snapshot, ...change };
    for (const listener of this.listeners) listener();
  }

  private start() {
    const generation = ++this.generation;
    void this.adapter.subscribe(this.attachment, () => { if (generation === this.generation) void this.refresh(); }).then((off) => {
      if (generation !== this.generation) return off();
      this.unlisten = off;
      void this.refresh();
    }, (error: unknown) => {
      if (generation !== this.generation) return;
      if (DEV_BUILD) console.debug("Background automation listener setup failed", error);
      this.publish({ loading: false, failed: true });
    });
  }

  refresh = async () => {
    if (!this.listeners.size) return;
    if (this.reading) { this.dirty = true; return; }
    const generation = this.generation;
    this.reading = true;
    this.dirty = false;
    const controller = new AbortController();
    this.controller = controller;
    try {
      const graph = scopedAutomationGraph(await this.adapter.read(this.attachment, controller.signal), this.attachment);
      if (generation === this.generation && !this.dirty) {
        // Graph events outpace graph changes; an identical read keeps the published snapshot for its subscribers.
        const same = !this.snapshot.loading && !this.snapshot.failed && sameJson(this.snapshot.graph, graph);
        if (!same) this.publish({ graph, loading: false, failed: false });
      }
    } catch (error) {
      if (generation !== this.generation || this.dirty) return;
      if (DEV_BUILD) console.debug("Background automation graph refresh failed", error);
      this.publish({ loading: false, failed: true });
    } finally {
      if (generation === this.generation) {
        this.reading = false;
        if (this.dirty) void this.refresh();
      }
    }
  };

  stop = async (identity: AutomationIdentity) => {
    const key = automationKey(identity);
    const node = this.snapshot.graph?.nodes.find((candidate) => automationKey(candidate.identity) === key);
    if (this.snapshot.failed || this.snapshot.stopping || !node?.stop.allowed || identity.kind !== "blueprint") return;
    const generation = this.generation;
    this.publish({ stopping: key, stopFailed: null });
    try {
      await this.adapter.stop(this.attachment, identity);
      if (generation === this.generation) await this.refresh();
    } catch {
      if (generation === this.generation) this.publish({ stopFailed: key });
    } finally {
      if (generation === this.generation) this.publish({ stopping: null });
    }
  };
}
