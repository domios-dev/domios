import assert from "node:assert/strict";
import {
  canSendText,
  isPtyBacked,
  pluginViewSurface,
  surfaceOwnerTitle,
  surfaceQuery,
  tabRowLabel,
  workspaceIdOfTab,
  isSurfaceTab,
  surfaceBacking,
  surfaceDisplayLabel,
  surfaceOwnerId,
  tabBody,
} from "./surfaceTab";
import type { SurfaceTabBacking, Tab } from "./types";

const backing: SurfaceTabBacking = {
  kind: "surface",
  plugin_id: "dash",
  surface_id: "board",
  instance_id: "inst-1",
  owner_label: true,
};
const base: Tab = { id: "t", title: "Board", tab_type: "Terminal", pty_id: null, cwd: null };
const surface: Tab = { ...base, backing, parent_tab_id: "owner" };
const chat: Tab = { ...base, backing: { kind: "chat_backend", plugin_id: "p", session_id: "s" } };
const pty: Tab = { ...base, backing: { kind: "pty" } };

assert.equal(surfaceBacking(surface), backing);
assert.equal(surfaceBacking(chat), null);
assert.equal(surfaceBacking(undefined), null);
assert.equal(isSurfaceTab(surface), true);
assert.equal(isSurfaceTab(base), false);

assert.equal(isPtyBacked(base), true, "a tab without backing is a PTY");
assert.equal(isPtyBacked(pty), true);
assert.equal(isPtyBacked(chat), false);
assert.equal(isPtyBacked(surface), false);

assert.equal(tabBody(surface), "surface_view");
assert.equal(tabBody(chat), "chat_backend");
assert.equal(tabBody(pty), "terminal");
assert.equal(tabBody(base), "terminal");

assert.equal(canSendText(surface), false);
assert.equal(canSendText(chat), true);
assert.equal(canSendText(pty), true);

assert.equal(surfaceDisplayLabel("Board", backing, "Jev"), "Jev's Board");
assert.equal(surfaceDisplayLabel("Board", backing, "  "), "Board", "a blank owner name falls back to the title");
assert.equal(surfaceDisplayLabel("Board", backing, null), "Board", "an owner that closed falls back to the title");
assert.equal(surfaceDisplayLabel("Board", { ...backing, owner_label: false }, "Jev"), "Board");

assert.equal(surfaceOwnerId(surface), "owner");
assert.equal(surfaceOwnerId(base), null);

const viewSurface = pluginViewSurface("tab 1", backing);
assert.deepEqual(viewSurface, { surfaceId: "board", instanceId: "inst-1", tabId: "tab 1" });
const query = new URLSearchParams(surfaceQuery(viewSurface).slice(1));
assert.equal(query.get("surface"), "board");
assert.equal(query.get("instance"), "inst-1");
assert.equal(query.get("tab"), "tab 1", "values round-trip through URL encoding");
assert.equal(surfaceQuery(undefined), "");

const workspaces = [{ id: "w1", tabs: ["a"] }, { id: "w2", tabs: ["b", "owner"] }];
assert.equal(workspaceIdOfTab(workspaces, "owner"), "w2");
assert.equal(workspaceIdOfTab(workspaces, "missing"), null);
assert.equal(workspaceIdOfTab(workspaces, null), null);

const tabs = { owner: { title: "Jev" } };
assert.equal(surfaceOwnerTitle(surface, tabs), "Jev");
assert.equal(surfaceOwnerTitle({ ...surface, backing: { ...backing, owner_label: false } }, tabs), null);
assert.equal(surfaceOwnerTitle({ ...surface, parent_tab_id: null }, tabs), null);
assert.equal(surfaceOwnerTitle(surface, {}), null, "a closed owner yields no title");
assert.equal(surfaceOwnerTitle(pty, tabs), null);
assert.equal(tabRowLabel(surface, "Board", "Jev"), surfaceDisplayLabel("Board", backing, "Jev"));
assert.equal(tabRowLabel(pty, "Board", "Jev"), "Board", "only surfaces compose an owner label");

console.log("surfaceTab: passed");
