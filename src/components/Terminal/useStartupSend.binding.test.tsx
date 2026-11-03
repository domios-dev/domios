import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import type { Tab } from "../../lib/types";
import { useStartupSend } from "./useStartupSend";
const state = { tabs: {}, workspaces: [], teams: [], generation: 1 } as unknown as ExtendedState;
let current = state;
const store = createMuxStateStore(() => current);
let allowed = true;
function Probe() { allowed = useStartupSend("target", false).allowed; return null; }
const root = createRoot(document.createElement("div"));
await act(async () => root.render(createElement(MuxStateStoreContext.Provider, { value: store }, createElement(Probe))));
assert.equal(allowed, false);
await act(async () => (current = { ...state, tabs: { target: { id: "target" } as Tab } }, store.notify()));
assert.equal(allowed, true);
await act(async () => (current = state, store.notify()));
assert.equal(allowed, false);
await act(async () => root.unmount());
console.log("PASS composer is disabled until its exact tab exists and on tab loss");

