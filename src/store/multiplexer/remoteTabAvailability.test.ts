import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { RemoteChannelAvailabilityMap } from "../../lib/remoteTabTransportState";
import { remoteChatUsable, remoteTerminalUsable } from "../../lib/remoteTabTransportState";
import type { Tab } from "../../lib/types";
import { remoteTabsFromOrdinaryProjection } from "./remoteTabProjection";

const projected = (channels: RemoteChannelAvailabilityMap): Tab => ({
  id: "mesh:peer/one", title: "One", pty_id: "mesh:peer/one", tab_type: "Terminal", cwd: null,
  remote_peer_id: "peer", remote_tab_id: "one", remote_channels: channels,
});

describe("per-channel remote availability aggregation", () => {
  it("chat channel failure does not disable terminal", () => {
    const channels: RemoteChannelAvailabilityMap = {
      control: "available",
      output: "available",
      chat: "unavailable",
    };
    expect(remoteTerminalUsable(channels)).toBe(true);
    expect(remoteChatUsable(channels)).toBe(false);
  });

  it("terminal stays usable while a channel is transiently reconnecting", () => {
    expect(remoteTerminalUsable({ control: "reconnecting", output: "available" })).toBe(true);
    expect(remoteTerminalUsable({ control: "available", output: "reconnecting" })).toBe(true);
  });

  it("an unavailable output or control channel disables the terminal", () => {
    expect(remoteTerminalUsable({ control: "available", output: "unavailable" })).toBe(false);
    expect(remoteTerminalUsable({ control: "unavailable", output: "available" })).toBe(false);
  });

  it("unknown channels stay optimistically usable", () => {
    expect(remoteTerminalUsable(undefined)).toBe(true);
    expect(remoteTerminalUsable({})).toBe(true);
    expect(remoteChatUsable(undefined)).toBe(true);
  });

  it("keeps one composite identity through per-channel reconnect and recovery", () => {
    const stages: RemoteChannelAvailabilityMap[] = [
      { control: "available", output: "available", chat: "available" },
      { control: "reconnecting", output: "reconnecting", chat: "reconnecting" },
      { control: "available", output: "available", chat: "unavailable" },
    ];
    for (const channels of stages) {
      const tabs = remoteTabsFromOrdinaryProjection([projected(channels)]);
      expect(Object.keys(tabs)).toEqual(["mesh:peer/one"]);
      expect(tabs["mesh:peer/one"].remote_channels).toEqual(channels);
    }
  });
});

finish();
