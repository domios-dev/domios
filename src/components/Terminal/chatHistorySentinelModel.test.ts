import { describe, expect, it } from "vitest";
import {
  createHistorySentinelController,
  HISTORY_SENTINEL_ROOT_MARGIN,
  type ChatHistorySentinelConfig,
  type HistorySentinelObserverFactory,
  type HistorySentinelObserverOptions,
} from "./chatHistorySentinelModel";

interface FakeObserverInstance {
  target: Element | null;
  options: HistorySentinelObserverOptions;
  disconnected: boolean;
  deliver(isIntersecting: boolean): void;
}

function createFakeObserverPort() {
  const instances: FakeObserverInstance[] = [];
  const createObserver: HistorySentinelObserverFactory = (callback, options) => {
    const entry: FakeObserverInstance = {
      target: null,
      options,
      disconnected: false,
      deliver(isIntersecting) {
        callback(isIntersecting);
      },
    };
    instances.push(entry);
    return {
      observe(target) {
        entry.target = target;
      },
      disconnect() {
        entry.disconnected = true;
      },
    };
  };
  return { createObserver, instances, latest: () => instances[instances.length - 1] };
}

const baseConfig: ChatHistorySentinelConfig = {
  active: true,
  hasMore: true,
  loading: false,
  paused: false,
  stalled: false,
};

describe("chatHistorySentinelModel", () => {
  it("triggers exactly one load per intersection reach and re-arms only after leaving", () => {
    const root = document.createElement("div");
    const target = document.createElement("div");
    const port = createFakeObserverPort();
    let loads = 0;

    const controller = createHistorySentinelController({
      root,
      target,
      config: baseConfig,
      onLoad: () => {
        loads++;
      },
      createObserver: port.createObserver,
    });

    expect(port.instances).toHaveLength(1);
    expect(port.latest().target).toBe(target);
    expect(port.latest().options).toEqual({
      root,
      rootMargin: HISTORY_SENTINEL_ROOT_MARGIN,
      threshold: 0,
    });

    port.latest().deliver(false);
    expect(loads).toBe(0);

    port.latest().deliver(true);
    expect(loads).toBe(1);

    port.latest().deliver(true);
    controller.update({ ...baseConfig, loading: true });
    controller.update({ ...baseConfig, loading: false });
    expect(loads).toBe(1);

    port.latest().deliver(false);
    expect(loads).toBe(1);

    port.latest().deliver(true);
    expect(loads).toBe(2);

    controller.dispose();
    expect(port.latest().disconnected).toBe(true);
  });

  it("never loops when a load finishes with nothing more prepended or when exhausted", () => {
    const root = document.createElement("div");
    const target = document.createElement("div");
    const port = createFakeObserverPort();
    let loads = 0;

    const controller = createHistorySentinelController({
      root,
      target,
      config: baseConfig,
      onLoad: () => {
        loads++;
      },
      createObserver: port.createObserver,
    });

    port.latest().deliver(true);
    expect(loads).toBe(1);

    // Load completes without pushing the sentinel out of view.
    controller.update({ ...baseConfig, loading: true });
    controller.update({ ...baseConfig, loading: false });
    port.latest().deliver(true);
    expect(loads).toBe(1);

    // Exhaustion disconnects the observer and ignores any late delivery.
    controller.update({ ...baseConfig, hasMore: false });
    expect(port.latest().disconnected).toBe(true);
    port.latest().deliver(false);
    port.latest().deliver(true);
    expect(loads).toBe(1);

    controller.dispose();
  });

  it("stops observing when hidden or inactive and ignores stale callbacks", () => {
    const root = document.createElement("div");
    const target = document.createElement("div");
    const port = createFakeObserverPort();
    let loads = 0;

    const controller = createHistorySentinelController({
      root,
      target,
      config: { ...baseConfig, active: false },
      onLoad: () => {
        loads++;
      },
      createObserver: port.createObserver,
    });

    expect(port.instances).toHaveLength(0);

    controller.update({ ...baseConfig, active: true });
    expect(port.instances).toHaveLength(1);
    const first = port.latest();

    controller.update({ ...baseConfig, active: false });
    expect(first.disconnected).toBe(true);
    first.deliver(true);
    expect(loads).toBe(0);

    controller.update({ ...baseConfig, active: true });
    expect(port.instances).toHaveLength(2);
    port.latest().deliver(true);
    expect(loads).toBe(1);

    controller.dispose();
  });

  it("defers a reach while a scroll gesture is paused and fires once when it settles", () => {
    const root = document.createElement("div");
    const target = document.createElement("div");
    const port = createFakeObserverPort();
    let loads = 0;

    const controller = createHistorySentinelController({
      root,
      target,
      config: { ...baseConfig, paused: true },
      onLoad: () => {
        loads++;
      },
      createObserver: port.createObserver,
    });

    port.latest().deliver(true);
    expect(loads).toBe(0);

    controller.update({ ...baseConfig, paused: false });
    expect(loads).toBe(1);

    // Still intersecting after settle must not trigger a second load.
    controller.update({ ...baseConfig, paused: false });
    expect(loads).toBe(1);

    // Leaving before the gesture settles cancels the deferred reach.
    port.latest().deliver(false);
    controller.update({ ...baseConfig, paused: true });
    port.latest().deliver(true);
    port.latest().deliver(false);
    controller.update({ ...baseConfig, paused: false });
    expect(loads).toBe(1);

    controller.dispose();
  });

  it("does not auto-load when stalled or immediately after an in-flight initial/manual load", () => {
    const root = document.createElement("div");
    const target = document.createElement("div");
    const port = createFakeObserverPort();
    let loads = 0;

    const controller = createHistorySentinelController({
      root,
      target,
      config: { ...baseConfig, loading: true },
      onLoad: () => {
        loads++;
      },
      createObserver: port.createObserver,
    });

    port.latest().deliver(true);
    expect(loads).toBe(0);
    controller.update({ ...baseConfig, loading: false });
    expect(loads).toBe(0);

    port.latest().deliver(false);
    controller.update({ ...baseConfig, stalled: true });
    port.latest().deliver(true);
    expect(loads).toBe(0);

    port.latest().deliver(false);
    controller.update({ ...baseConfig, stalled: false });
    port.latest().deliver(true);
    expect(loads).toBe(1);

    controller.dispose();
  });
});
