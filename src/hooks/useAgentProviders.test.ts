// Run: npx tsx src/hooks/useAgentProviders.test.ts
// Regression for GH #185: secondary useAgentProviders mounts must not wipe the
// shared descriptor cache while their fetch is still in flight.

import { getAgentDescriptor, listAgentDescriptors, setAgentDescriptors } from "../lib/agentDescriptorCache";
import type { ProviderDescriptor } from "../lib/types";
import {
  attachModelsUpdatedRefresh,
  shouldRefreshAgentProviders,
  syncAgentDescriptorCache,
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

test("sync skips while loading so a mounting hook cannot clear warm cache", () => {
  setAgentDescriptors([provider("claude")]);
  syncAgentDescriptorCache([], true);
  assert(getAgentDescriptor("claude")?.id === "claude", "warm cache survives in-flight fetch");
});

test("sync publishes once loading finishes", () => {
  syncAgentDescriptorCache([provider("codex")], false);
  assert(getAgentDescriptor("codex")?.id === "codex", "loaded providers reach cache");
});

test("sync allows empty registry after a completed load", () => {
  syncAgentDescriptorCache([], false);
  assert(getAgentDescriptor("codex") === undefined, "empty completed fetch clears cache");
});

test("models-updated refresh is wired alongside contributions changes", () => {
  assert(shouldRefreshAgentProviders("contributions-changed"), "contribution event should refresh");
  assert(shouldRefreshAgentProviders("models-updated"), "models-updated should refresh");
  assert(!shouldRefreshAgentProviders("tab-focused"), "irrelevant events must not refresh");
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
