// Run: npx tsx src/store/multiplexer/tabRename.test.ts
import { strict as assert } from "node:assert";
import type { Notice } from "../../lib/notices";
import { TitleLevel, type Tab, type Workspace } from "../../lib/types";
import { reducer, type Action } from "./reducer";
import { commitTabRename } from "./tabRename";
import type { ExtendedState } from "./tree";

function state(): ExtendedState {
  return {
    workspaces: [{ id: "ws", name: "W", tabs: ["tab"], active_tab_id: "tab", cwd: null } as unknown as Workspace],
    tabs: { tab: { id: "tab", pty_id: "pty", title: "Bizet", title_level: TitleLevel.Auto } as unknown as Tab },
    teams: [],
    active_workspace_id: "ws",
    focused_tab_id: "tab",
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 0,
    pendingCloses: [],
  };
}

function harness(persist: () => Promise<unknown>) {
  let current = state();
  const persisted: unknown[] = [];
  const notices: Notice[] = [];
  const ports = {
    dispatch: (action: Action) => { current = reducer(current, action); },
    persist: (args: unknown) => { persisted.push(args); return persist(); },
    notify: (notice: Notice) => { notices.push(notice); },
  };
  return { ports, persisted, notices, get state() { return current; }, apply: (action: Action) => ports.dispatch(action) };
}

async function main() {
  {
    const h = harness(() => Promise.resolve({ ok: true }));
    const pending = commitTabRename(h.state, h.ports, "ws", "tab", "Renamed");
    assert.equal(h.state.tabs.tab.title, "Renamed", "the new title shows before the daemon answers");
    await pending;
    assert.deepEqual(h.persisted, [{ workspaceId: "ws", tabId: "tab", title: "Renamed" }], "one persist carries the title");
    assert.equal(h.state.tabs.tab.title_level, TitleLevel.User);
    assert.deepEqual(h.notices, [], "a saved rename raises no notice");
  }

  {
    const h = harness(() => Promise.reject(new Error("HTTP 422: missing field `title`")));
    await commitTabRename(h.state, h.ports, "ws", "tab", "Renamed");
    const tab = h.state.tabs.tab;
    assert.equal(tab.title, "Bizet", "a refused rename restores the previous title");
    assert.equal(tab.title_level, TitleLevel.Auto, "a refused rename restores the previous title level");
    assert.equal(tab.title_pending_local, false, "the next snapshot is authoritative again");
    assert.equal(h.notices.length, 1, "a refused rename is never silent");
    const notice = h.notices[0];
    assert.equal(notice.severity, "Error");
    assert.equal(notice.title, "Could not rename the tab");
    assert.equal(notice.tabId, "tab");
    assert.match(notice.detail ?? "", /missing field/, "the daemon's reason is kept for the reader");
  }

  {
    let reject!: (error: unknown) => void;
    const h = harness(() => new Promise((_, fail) => { reject = fail; }));
    const pending = commitTabRename(h.state, h.ports, "ws", "tab", "First");
    h.apply({ type: "SET_TAB_TITLE", tabId: "tab", title: "Second", level: TitleLevel.User });
    reject(new Error("refused"));
    await pending;
    assert.equal(h.state.tabs.tab.title, "Second", "a refused rename never rolls back a newer title");
  }

  {
    const h = harness(() => Promise.resolve());
    await commitTabRename(h.state, h.ports, "other", "tab", "Renamed");
    assert.deepEqual(h.persisted, [], "a tab outside the workspace is not renamed");
    assert.equal(h.state.tabs.tab.title, "Bizet");
  }

  console.log("tabRename.test.ts: ok");
}

void main().catch((error) => {
  console.error(error);
  process.exit(1);
});
