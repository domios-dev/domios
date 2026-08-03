import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { Tab } from "../../lib/types";
import { remoteTabSubtitle } from "./TabItem";

const remote = (role?: Tab["role"], roleProfile?: string): Tab => ({
  id: "mesh:peer/one",
  title: "One",
  pty_id: "one",
  tab_type: "Terminal",
  cwd: null,
  role,
  role_profile: roleProfile,
  remote_peer_id: "peer",
  remote_peer_name: "ROG",
  remote_tab_id: "one",
  remote_controller_title: "Hubble",
  remote_availability: "available",
});

finish();

describe("remote tab subtitle", () => {
  it("shows one canonical managed-by attribution without repeating the peer", () => {
    expect(remoteTabSubtitle(remote("Manager", "planner"), "PLANNER")).toBe("managed by Hubble");
    expect(remoteTabSubtitle(remote("Orchestrator"), "ORCHESTRATOR")).toBe("managed by Hubble");
    expect(remoteTabSubtitle({ ...remote(), remote_controller_title: undefined }, null))
      .toBe("managed by @ROG");
    expect(remoteTabSubtitle({ ...remote(), remote_availability: "reconnecting" }, null))
      .toBe("managed by Hubble · Reconnecting");
  });

  it("leaves local subtitle byte-for-byte unchanged", () => {
    const local = { ...remote("Manager", "planner"), remote_peer_name: undefined };
    expect(remoteTabSubtitle(local, "PLANNER")).toBe("PLANNER");
  });
});
