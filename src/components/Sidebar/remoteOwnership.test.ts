import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { Tab } from "../../lib/types";
import { remoteOwnership, remoteOwnershipSubtitle } from "./remoteOwnership";

function remote(overrides: Partial<Tab>): Tab {
  return {
    id: "mesh:peer/worker",
    title: "Worker",
    pty_id: "mesh:peer/worker",
    remote_peer_id: "peer",
    remote_peer_name: "Mac-Air",
    remote_tab_id: "worker",
    remote_availability: "available",
    ...overrides,
  } as unknown as Tab;
}

describe("canonical remote ownership presentation", () => {
  it("attributes to the controller without repeating the peer", () => {
    const owned = remoteOwnership(remote({ remote_controller_title: "Puccini" }))!;
    expect(owned.managedByLabel).toEqual("managed by Puccini");
    expect(owned.managedByLabel.includes("@")).toEqual(false);
    expect(remoteOwnershipSubtitle(owned)).toEqual("managed by Puccini");
  });

  it("falls back to the peer only when no controller disambiguates", () => {
    const owned = remoteOwnership(remote({ remote_controller_title: undefined }))!;
    expect(owned.managedByLabel).toEqual("managed by @Mac-Air");
  });

  it("appends one availability note", () => {
    const owned = remoteOwnership(
      remote({ remote_controller_title: "Puccini", remote_availability: "reconnecting" }),
    )!;
    expect(remoteOwnershipSubtitle(owned)).toEqual("managed by Puccini · Reconnecting");
  });

  it("is null for local tabs", () => {
    expect(remoteOwnership({ id: "local", title: "x" } as unknown as Tab)).toEqual(null);
  });
});

finish();
