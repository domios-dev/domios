import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { Tab } from "../../lib/types";
import { remoteTabsFromOrdinaryProjection } from "./remoteTabProjection";

const tab = (id: string, remote = false): Tab => ({
  id, title: id, pty_id: id, tab_type: "Terminal", cwd: null,
  ...(remote ? { remote_peer_id: "peer", remote_tab_id: "shared-tab", remote_channels: { control: "available", output: "available", chat: "available" } as const } : {}),
});

finish();

describe("authoritative ordinary tab projection", () => {
  it("preserves native order and only selects native-projected remotes", () => {
    const tabs = [tab("local"), tab("mesh:peer/shared-tab", true)];
    expect(Object.keys(remoteTabsFromOrdinaryProjection(tabs))).toEqual(["mesh:peer/shared-tab"]);
  });
});
