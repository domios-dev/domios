// Run: npx tsx src/hooks/useAgentProviders.test.ts

import { agentDescriptorsRefreshing, acceptModelCatalogueChange, getAgentDescriptor, listAgentDescriptors, setAgentDescriptors } from "../lib/agentDescriptorCache";
import type { ProviderDescriptor } from "../lib/types";
import {
  agentProvidersFromResponse,
  attachModelCatalogueChangedRefresh,
  shouldRefreshAgentProviders,
  syncAgentDescriptorCache,
  visibleAgentProviders,
} from "./useAgentProviders";

let failed = 0;

const tests: Array<[string, () => void | Promise<void>]> = [];
function test(name: string, fn: () => void | Promise<void>) {
  tests.push([name, fn]);
}

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

function provider(id: string): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name: id,
    transport: "pty",
    system_prompt_delivery: "external",
    available_models: [],
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
    supports_chat_ui: true,
    icon_hint: id.slice(0, 2).toUpperCase(),
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

test("sync retains the catalog during refresh and errors", () => {
  const catalogA = [provider("catalog-a")];
  const catalogB = [provider("catalog-b")];

  setAgentDescriptors(catalogA);
  syncAgentDescriptorCache(catalogA, true, null);
  assert(getAgentDescriptor("catalog-a")?.id === "catalog-a", "refresh retains the visible catalog");
  assert(agentDescriptorsRefreshing(), "preset consumers can observe refresh without losing provider identity");

  syncAgentDescriptorCache(catalogA, false, "catalog unavailable");
  assert(getAgentDescriptor("catalog-a")?.id === "catalog-a", "fetch errors retain the last good catalogue");
  assert(!agentDescriptorsRefreshing(), "settled errors end the refresh state");

  syncAgentDescriptorCache(catalogB, false, null);
  assert(getAgentDescriptor("catalog-b")?.id === "catalog-b", "settled catalog B reaches cache consumers");
});

test("sync publishes once loading finishes", () => {
  syncAgentDescriptorCache([provider("loaded-provider")], false, null);
  assert(getAgentDescriptor("loaded-provider")?.id === "loaded-provider", "loaded providers reach cache");
});

test("sync allows empty registry after a completed load", () => {
  syncAgentDescriptorCache([], false, null);
  assert(getAgentDescriptor("loaded-provider") === undefined, "empty completed fetch clears cache");
});

test("catalogue version changes refresh provider descriptors", () => {
  assert(shouldRefreshAgentProviders("contributions-changed"), "contribution event should refresh");
  assert(shouldRefreshAgentProviders("model-catalogue-changed"), "catalogue change should refresh");
  assert(!shouldRefreshAgentProviders("tab-focused"), "irrelevant events must not refresh");
});

test("refresh and errors preserve catalogue identity", () => {
  const stale = [provider("stale-provider")];
  assert(visibleAgentProviders(stale, true, null) === stale, "refresh preserves visible entries");
  assert(visibleAgentProviders(stale, false, "catalog unavailable") === stale, "errors retain entries");
  assert(
    visibleAgentProviders(stale, true, "failed") === stale,
    "failed catalogs keep stable identity across consumer renders",
  );
  assert(visibleAgentProviders(stale, false, null) === stale, "settled catalog preserves identity");
});

test("malformed catalog responses fail visibly instead of hiding descriptors", () => {
  for (const malformed of [{}, null, { ok: true, data: [] }]) {
    let refused = false;
    try { agentProvidersFromResponse(malformed); } catch { refused = true; }
    assert(refused, "invalid catalog must report an error");
  }
  const catalog = [provider("catalog-provider")];
  assert(agentProvidersFromResponse(catalog) === catalog, "array response preserves catalog identity");
});

test("catalogue listener refetches a newer provider version while mounted", () => {
  let refetches = 0;
  let trigger: ((version: number) => void) | undefined;
  setAgentDescriptors([provider("codex")]);
  const cleanup = attachModelCatalogueChangedRefresh(
    () => {
      refetches += 1;
    },
    async <T>(event: string, handler: (event: { payload: T }) => void) => {
      assert(event === "model-catalogue-changed", "listener must use the versioned event");
      trigger = (version) => handler({ payload: { provider: "codex", version } as T });
      return () => {
        trigger = undefined;
      };
    },
  );

  trigger?.(1);
  cleanup();

  assert(refetches === 1, "listener should refetch while active");
});

test("catalogue events reach every mounted consumer and deduplicate per consumer", () => {
  setAgentDescriptors([provider("shared-catalogue")]);
  const handlers: Array<(event: {payload: {provider: string; version: number}}) => void> = [];
  const counts = [0, 0];
  const listen = async <T>(_event: string, handler: (event: {payload: T}) => void) => {
    handlers.push(handler as typeof handlers[number]);
    return () => {};
  };
  const cleanups = counts.map((_, index) => attachModelCatalogueChangedRefresh(() => counts[index]++, listen));
  for (const version of [1, 1, 2, 1]) {
    for (const handler of handlers) handler({payload: {provider: "shared-catalogue", version}});
  }
  assert(counts[0] === 2 && counts[1] === 2, "each consumer observes both newer versions once");
  cleanups.forEach((cleanup) => cleanup());
});
test("catalogue listener releases a late registration after cleanup", async () => {
  let refetches = 0;
  let unlistenCalls = 0;
  const registration = deferred<() => void>();
  const unlistenObserved = deferred<void>();

  const cleanup = attachModelCatalogueChangedRefresh(() => {
    refetches += 1;
  }, async () => registration.promise);

  cleanup();

  registration.resolve(() => {
    unlistenCalls += 1;
    unlistenObserved.resolve();
  });
  await unlistenObserved.promise;

  assert(unlistenCalls === 1, "late listener registration must be unhooked immediately");
  assert(refetches === 0, "cleanup must block late events");
});

test("catalogue listener swallows a rejected registration", async () => {
  let refetches = 0;
  let caught = false;
  const originalUnhandled = process.listeners("unhandledRejection");
  const unhandled = (reason: unknown) => {
    caught = true;
    throw reason;
  };
  process.removeAllListeners("unhandledRejection");
  process.on("unhandledRejection", unhandled);

  attachModelCatalogueChangedRefresh(
    () => {
      refetches += 1;
    },
    async () => {
      throw new Error("reconnect failed");
    },
  );

  await Promise.resolve();
  process.removeListener("unhandledRejection", unhandled);
  for (const listener of originalUnhandled) {
    process.on("unhandledRejection", listener);
  }

  assert(!caught, "registration rejection must not surface as unhandled");
  assert(refetches === 0, "registration failure must not refetch");
});

test("catalogue listener ignores duplicate and older versions", () => {
  setAgentDescriptors([{ ...provider("versioned"), catalogue_version: 4 }]);
  assert(!acceptModelCatalogueChange({ provider: "versioned", version: 4 }), "same version is cached");
  assert(!acceptModelCatalogueChange({ provider: "versioned", version: 3 }), "older version is ignored");
  assert(acceptModelCatalogueChange({ provider: "versioned", version: 5 }), "new version invalidates the cache");
});

test("provider cache does not roll back after a newer catalogue event", () => {
  const current = { ...provider("monotonic"), catalogue_version: 2 };
  setAgentDescriptors([current]);
  assert(acceptModelCatalogueChange({ provider: "monotonic", version: 3 }), "new catalogue version is accepted");

  setAgentDescriptors([{ ...current, available_models: [{ id: "stale", display_name: "Stale" }] }]);

  assert(
    getAgentDescriptor("monotonic")?.available_models.length === 0,
    "a stale in-flight response must not replace the cached catalogue",
  );
  assert(!acceptModelCatalogueChange({ provider: "monotonic", version: 3 }), "the event remains deduplicated");
});

test("listAgentDescriptors preserves registry order and identity", () => {
  const list = [provider("order-first"), provider("order-second")];
  setAgentDescriptors(list);
  const a = listAgentDescriptors();
  const b = listAgentDescriptors();
  assert(a === b, "stable identity between calls");
  assert(a.map((p) => p.id).join(",") === "order-first,order-second", "preserves order");
});

(async () => {
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`✓ ${name}`);
    } catch (err) {
      failed += 1;
      console.error(`✗ ${name}`);
      console.error(err);
    }
  }
  console.log(`useAgentProviders: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  if (failed > 0) process.exit(1);
})();
