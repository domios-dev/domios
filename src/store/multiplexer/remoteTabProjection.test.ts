import assert from "node:assert/strict";
import { test } from "node:test";
import type { Tab } from "../../lib/types";
import { shouldRefreshRemoteTabs } from "./remoteTabProjection";

const local: Tab = { id: "local", pty_id: null, tab_type: "Terminal", title: "Local", cwd: null };
const remote: Tab = { ...local, id: "remote", remote_peer_id: "peer" };

test("confirmed local-only inventory needs no ordinary tab projection", () => {
  assert.equal(shouldRefreshRemoteTabs(false, { local }, {}), false);
});

test("paired peers and unknown inventory retain remote discovery", () => {
  assert.equal(shouldRefreshRemoteTabs(true, { local }), true);
  assert.equal(shouldRefreshRemoteTabs(null, { local }), true);
});

test("remote tabs and saved attachments refresh after peers disappear", () => {
  assert.equal(shouldRefreshRemoteTabs(false, { remote }), true);
  assert.equal(shouldRefreshRemoteTabs(false, { local }, { remote }), true);
});
