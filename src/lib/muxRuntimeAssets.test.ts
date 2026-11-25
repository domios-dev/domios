import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { hydrateMuxSnapshot } from "./muxRuntimeAssets";
import type { MultiplexerState } from "./types";

describe("hydrateMuxSnapshot", () => {
  it("returns snapshots without runtime_assets unchanged by reference", () => {
    const snapshot: MultiplexerState = {
      generation: 3,
      workspaces: [],
      tabs: {
        a: { id: "a", pty_id: null, tab_type: "Terminal", title: "A", cwd: null },
      },
      active_workspace_id: null,
      focused_tab_id: null,
    };
    assert.equal(hydrateMuxSnapshot(snapshot), snapshot);
  });

  it("rehydrates hoisted icons, authentication, controls, configOptions and service tiers while sharing references", () => {
    const sharedCommands = [{ name: "plan", description: "Plan work", argumentHint: "[topic]" }];
    const sharedModes = [{ id: "code", name: "Code" }];
    const sharedAuth = { state: "authenticated" as const, methods: [{ id: "oauth", name: "OAuth", available: true }] };
    const sharedTiers = { "model-x": [{ id: "fast", name: "Fast", description: "Fast tier" }] };
    const sharedOptions = [
      { value: "m1", name: "Model 1" },
      { value: "m2", name: "Model 2" },
    ];

    const wire = {
      generation: 9,
      workspaces: [],
      active_workspace_id: null,
      focused_tab_id: null,
      tabs: {
        primary1: {
          id: "primary1",
          pty_id: null,
          tab_type: "Terminal" as const,
          title: "Primary 1",
          cwd: null,
          agent_runtime_state: {
            state: "loaded" as const,
            epoch: 2,
            iconRef: "ic1",
            authenticationRef: "au1",
            controls: {
              model: "m1",
              commandsRef: "cm1",
              modesRef: "md1",
              configOptionsRef: "co1",
              configOptionValues: ["m1", "high"],
            },
          },
        },
        primary2: {
          id: "primary2",
          pty_id: null,
          tab_type: "Terminal" as const,
          title: "Primary 2",
          cwd: null,
          agent_runtime_state: {
            state: "loaded" as const,
            epoch: 4,
            iconRef: "ic1",
            authenticationRef: "au1",
            controls: {
              model: "m2",
              commandsRef: "cm1",
              modesRef: "md1",
              configOptionsRef: "co1",
              configOptionValues: ["m2", "low"],
            },
          },
        },
        tiered1: {
          id: "tiered1",
          pty_id: null,
          tab_type: "Terminal" as const,
          title: "Tiered",
          cwd: null,
          agent_runtime_config: {
            model: "model-x",
            serviceTierAvailabilityRef: "st1",
          },
          agent_runtime_state: {
            state: "loaded" as const,
            epoch: 1,
            controls: {
              model: "model-x",
              serviceTierAvailabilityRef: "st1",
            },
          },
        },
        plain: {
          id: "plain",
          pty_id: null,
          tab_type: "Terminal" as const,
          title: "Plain",
          cwd: null,
        },
      },
      runtime_assets: {
        icons: { ic1: "<svg><path/></svg>" },
        authentication: { au1: sharedAuth },
        commands: { cm1: sharedCommands },
        modes: { md1: sharedModes },
        configOptions: {
          co1: [
            { id: "model", name: "Model", category: "model", options: sharedOptions },
            { id: "effort", name: "Effort", category: "thought_level", options: [{ value: "low", name: "Low" }, { value: "high", name: "High" }] },
          ],
        },
        serviceTierAvailability: { st1: sharedTiers },
      },
    };

    const hydrated = hydrateMuxSnapshot(wire as unknown as MultiplexerState);
    assert.equal("runtime_assets" in hydrated, false);
    assert.equal(hydrated.tabs.plain, wire.tabs.plain);

    const primary1 = hydrated.tabs.primary1.agent_runtime_state!;
    const primary2 = hydrated.tabs.primary2.agent_runtime_state!;
    assert.equal(primary1.iconSvg, "<svg><path/></svg>");
    assert.equal(primary2.iconSvg, primary1.iconSvg);
    assert.equal("iconRef" in primary1, false);
    assert.equal(primary1.authentication, sharedAuth);
    assert.equal(primary2.authentication, sharedAuth);
    assert.equal(primary1.controls?.commands, sharedCommands);
    assert.equal(primary2.controls?.commands, sharedCommands);
    assert.equal(primary1.controls?.modes, sharedModes);
    assert.equal(primary2.controls?.modes, sharedModes);
    assert.equal("commandsRef" in (primary1.controls ?? {}), false);
    assert.equal("configOptionsRef" in (primary1.controls ?? {}), false);
    assert.equal("configOptionValues" in (primary1.controls ?? {}), false);
    assert.deepEqual(primary1.controls?.configOptions, [
      { id: "model", name: "Model", category: "model", currentValue: "m1", options: sharedOptions },
      { id: "effort", name: "Effort", category: "thought_level", currentValue: "high", options: [{ value: "low", name: "Low" }, { value: "high", name: "High" }] },
    ]);
    assert.equal(primary2.controls?.configOptions?.[0]?.currentValue, "m2");
    assert.equal(primary2.controls?.configOptions?.[1]?.currentValue, "low");
    // Shared options array reference avoids duplicating option lists in memory.
    assert.equal(primary1.controls?.configOptions?.[0]?.options, sharedOptions);
    assert.equal(primary2.controls?.configOptions?.[0]?.options, sharedOptions);

    const tiered = hydrated.tabs.tiered1;
    assert.equal(tiered.agent_runtime_config?.serviceTierAvailability, sharedTiers);
    assert.equal(tiered.agent_runtime_state?.controls?.serviceTierAvailability, sharedTiers);
    assert.equal("serviceTierAvailabilityRef" in (tiered.agent_runtime_config ?? {}), false);
    assert.equal("serviceTierAvailabilityRef" in (tiered.agent_runtime_state?.controls ?? {}), false);
  });
});
