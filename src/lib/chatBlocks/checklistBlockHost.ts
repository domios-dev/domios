import type { ChecklistBlockHost, ChecklistBlockState } from "@codeterm/chat-ui";
import { buildCatalog } from "../../components/Extensions/pluginCatalog";
import type { PushChannelDescriptor } from "../notifyDestinations";
import type { PluginSnapshotStoreState } from "../pluginSnapshotStore";
import type { MultiplexerState } from "../types";
import {
  agentSpawned,
  blueprintCreated,
  checklistInfo,
  CHECKLISTS,
  memorySaved,
  phoneConnected,
  pluginInstalled,
  type SetupLink,
  type SetupSignals,
} from "./checklists";
import type { JobsBlockStore } from "./jobsBlockStore";

interface Source<T> {
  get(): T;
  subscribe(listener: () => void): () => void;
}

export interface ChecklistSources {
  mux: Source<Pick<MultiplexerState, "tabs">>;
  plugins: Source<PluginSnapshotStoreState> & { ensure(): Promise<unknown> };
  jobs: JobsBlockStore;
  readChannels(): Promise<readonly PushChannelDescriptor[]>;
  readMemoryKinds(): Promise<Readonly<Record<string, number>>>;
  links: Record<SetupLink, () => void>;
}

/** Checklist items tick from the mux, plugin, jobs, notification-channel and memory state; reads never mutate. */
export function createChecklistBlockHost(sources: ChecklistSources): ChecklistBlockHost {
  let channels: readonly PushChannelDescriptor[] | null = null;
  let memoryKinds: Readonly<Record<string, number>> | null = null;
  let cachedKey: unknown[] = [];
  const states = new Map<string, ChecklistBlockState>();
  const listeners = new Set<() => void>();
  const notify = () => {
    for (const listener of [...listeners]) listener();
  };

  const signals = (): SetupSignals => {
    const plugins = sources.plugins.get();
    return {
      agentSpawned: agentSpawned(sources.mux.get().tabs),
      phoneConnected: phoneConnected(channels),
      pluginInstalled: pluginInstalled(plugins.hasLoaded, buildCatalog(plugins.snapshot)),
      blueprintCreated: blueprintCreated(sources.jobs.get()),
      memorySaved: memorySaved(memoryKinds),
    };
  };

  const refresh = () => {
    void sources.plugins.ensure().catch(() => {});
    void sources.jobs.refresh();
    void sources.readChannels().then((next) => {
      channels = next;
      notify();
    }, () => {});
    void sources.readMemoryKinds().then((next) => {
      memoryKinds = next;
      notify();
    }, () => {});
  };

  return {
    binding: {
      subscribe(listener) {
        listeners.add(listener);
        if (listeners.size === 1) window.addEventListener("focus", refresh);
        const stops = [sources.mux.subscribe(listener), sources.plugins.subscribe(listener), sources.jobs.subscribe(listener)];
        return () => {
          listeners.delete(listener);
          for (const stop of stops) stop();
          if (listeners.size === 0) window.removeEventListener("focus", refresh);
        };
      },
      read(checklistId) {
        const key = [sources.mux.get().tabs, sources.plugins.get(), sources.jobs.get(), channels, memoryKinds];
        if (key.some((part, index) => part !== cachedKey[index])) {
          cachedKey = key;
          states.clear();
        }
        let state = states.get(checklistId);
        if (!state) {
          const definition = CHECKLISTS[checklistId];
          state = definition ? { kind: "ready", checklist: checklistInfo(definition, signals()) } : { kind: "missing" };
          states.set(checklistId, state);
        }
        return state;
      },
      refresh,
    },
    open(checklistId, itemId) {
      const item = CHECKLISTS[checklistId]?.items.find((candidate) => candidate.id === itemId);
      if (item?.action.kind === "link") sources.links[item.action.link]();
    },
  };
}
