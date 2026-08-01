// Run: npx tsx src/hooks/useAgentProviders.test.ts

import { getAgentDescriptor, listAgentDescriptors, setAgentDescriptors } from "../lib/agentDescriptorCache";
import type { ProviderDescriptor } from "../lib/types";
import {
  agentProvidersFromResponse,
  attachModelsUpdatedRefresh,
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

test("sync hides the previous catalog throughout loading and error before publishing the next catalog", () => {
  const catalogA = [provider("catalog-a")];
  const catalogB = [provider("catalog-b")];

  setAgentDescriptors(catalogA);
  syncAgentDescriptorCache(catalogA, true, null);
  assert(listAgentDescriptors().length === 0, "loading clears catalog A from cache consumers");

  syncAgentDescriptorCache(catalogA, false, "catalog unavailable");
  assert(listAgentDescriptors().length === 0, "fetch errors cannot republish retained catalog A items");

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

test("models-updated refresh is wired alongside contributions changes", () => {
  assert(shouldRefreshAgentProviders("contributions-changed"), "contribution event should refresh");
  assert(shouldRefreshAgentProviders("models-updated"), "models-updated should refresh");
  assert(!shouldRefreshAgentProviders("tab-focused"), "irrelevant events must not refresh");
});

test("loading or failed catalogs expose no stale provider entries", () => {
  const stale = [provider("stale-provider")];
  assert(visibleAgentProviders(stale, true, null).length === 0, "loading hides stale entries");
  assert(visibleAgentProviders(stale, false, "catalog unavailable").length === 0, "errors hide stale entries");
  assert(visibleAgentProviders(stale, false, null) === stale, "settled catalog preserves identity");
});

test("malformed catalog responses expose an honest empty catalog", () => {
  assert(agentProvidersFromResponse({}).length === 0, "object response is not a provider catalog");
  assert(agentProvidersFromResponse(null).length === 0, "null response is not a provider catalog");
  const catalog = [provider("catalog-provider")];
  assert(agentProvidersFromResponse(catalog) === catalog, "array response preserves catalog identity");
});

test("models-updated listener refetches while mounted", () => {
  let refetches = 0;
  let trigger: (() => void) | undefined;
  const cleanup = attachModelsUpdatedRefresh(
    () => {
      refetches += 1;
    },
    async <T>(_event: string, handler: (event: { payload: T }) => void) => {
      trigger = () => handler({ payload: { provider: "codex" } as T });
      return () => {
        trigger = undefined;
      };
    },
  );

  trigger?.();
  cleanup();

  assert(refetches === 1, "listener should refetch while active");
});

test("models-updated listener releases a late registration after cleanup", async () => {
  let refetches = 0;
  let unlistenCalls = 0;
  const registration = deferred<() => void>();
  const unlistenObserved = deferred<void>();

  const cleanup = attachModelsUpdatedRefresh(() => {
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

test("models-updated listener swallows a rejected registration", async () => {
  let refetches = 0;
  let caught = false;
  const originalUnhandled = process.listeners("unhandledRejection");
  const unhandled = (reason: unknown) => {
    caught = true;
    throw reason;
  };
  process.removeAllListeners("unhandledRejection");
  process.on("unhandledRejection", unhandled);

  attachModelsUpdatedRefresh(
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

test("listAgentDescriptors preserves registry order and identity", () => {
  const list = [provider("claude"), provider("codex")];
  setAgentDescriptors(list);
  const a = listAgentDescriptors();
  const b = listAgentDescriptors();
  assert(a === b, "stable identity between calls");
  assert(a.map((p) => p.id).join(",") === "claude,codex", "preserves order");
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
