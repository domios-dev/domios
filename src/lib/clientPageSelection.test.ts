import assert from "node:assert/strict";
import { Window } from "happy-dom";
import { loadClientPageSelection, saveClientPageSelection } from "./clientPageSelection";

const browser = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: browser, configurable: true });
Object.defineProperty(globalThis, "localStorage", { value: browser.localStorage, configurable: true });
const peerKey = `codeterm-web-relay-peer:${browser.location.origin}`;
assert.equal(loadClientPageSelection(), null);
saveClientPageSelection({ workspaceId: "own-workspace", tabId: "own-tab" });
assert.deepEqual(loadClientPageSelection(), { workspaceId: "own-workspace", tabId: "own-tab" });
browser.localStorage.setItem(peerKey, "remote");
assert.equal(loadClientPageSelection(), null);
saveClientPageSelection({ workspaceId: "peer-workspace", tabId: "peer-tab" });
browser.localStorage.removeItem(peerKey);
assert.deepEqual(loadClientPageSelection(), { workspaceId: "own-workspace", tabId: "own-tab" });
browser.localStorage.setItem(peerKey, "remote");
assert.deepEqual(loadClientPageSelection(), { workspaceId: "peer-workspace", tabId: "peer-tab" });
console.log("client page selection storage checks passed");
