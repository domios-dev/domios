import { describe, expect, it } from "vitest";
import type { SurfaceTabBacking, Tab } from "../../lib/types";
import { partitionTeamMembers } from "./workspaceTeamModel";
import {
  ATTACHMENT_AVATAR_INDENT,
  attachmentBlockModel,
  attachmentInset,
  collapsedAttachmentBadge,
  ownerRowInset,
  surfaceAttachmentsByOwner,
} from "./surfaceAttachmentModel";

const backing: SurfaceTabBacking = { kind: "surface", plugin_id: "browser", surface_id: "page", instance_id: "i", owner_label: true };

function tab(id: string, extra: Partial<Tab> = {}): Tab {
  return { id, title: id, tab_type: "Terminal", pty_id: null, cwd: null, ...extra };
}

const surface = (id: string, owner: string) => tab(id, { backing, parent_tab_id: owner });

describe("surface attachment grouping", () => {
  it("groups each surface under its owner in tab order", () => {
    const tabs = [tab("snake"), surface("s1", "snake"), tab("jev"), surface("s2", "snake"), surface("s3", "jev")];
    const grouped = surfaceAttachmentsByOwner(tabs);
    expect([...grouped.keys()]).toEqual(["snake", "jev"]);
    expect(grouped.get("snake")?.map((t) => t.id)).toEqual(["s1", "s2"]);
    expect(grouped.get("jev")?.map((t) => t.id)).toEqual(["s3"]);
  });

  it("leaves orphaned surfaces and ordinary children out", () => {
    const tabs = [tab("snake"), surface("orphan", "gone"), tab("child", { parent_tab_id: "snake" })];
    expect(surfaceAttachmentsByOwner(tabs).size).toBe(0);
  });

  it("never hangs a surface under another surface", () => {
    const tabs = [tab("snake"), surface("s1", "snake"), surface("nested", "s1")];
    const grouped = surfaceAttachmentsByOwner(tabs);
    expect([...grouped.keys()]).toEqual(["snake"]);
    expect(grouped.get("snake")?.map((t) => t.id)).toEqual(["s1"]);
  });

  it("tucks a Worker's surface under that Worker, one avatar in from the Worker's indent", () => {
    const members = [
      tab("mgr", { role: "Manager", team_id: "t" }),
      tab("worker", { role: "Worker", team_id: "t", parent_tab_id: "mgr" }),
      surface("browser", "worker"),
    ].map((t, index) => ({ tab: { ...t, team_id: "t" }, index }));
    const { surfacesByOwner, terminals } = partitionTeamMembers(members);
    expect(surfacesByOwner.get("worker")?.map((m) => m.tab.id)).toEqual(["browser"]);
    expect(surfacesByOwner.has("mgr")).toBe(false);
    expect(terminals).toEqual([]);
    expect(attachmentInset(ownerRowInset("team", false))).toBe(ownerRowInset("team", false) + ATTACHMENT_AVATAR_INDENT);
  });

  it("indents loose attachments from the loose row inset, touch included", () => {
    expect(attachmentInset(ownerRowInset("loose", false))).toBe(16 + ATTACHMENT_AVATAR_INDENT);
    expect(attachmentInset(ownerRowInset("loose", true))).toBe(20 + ATTACHMENT_AVATAR_INDENT);
  });
});

describe("collapsed owner badge", () => {
  it("shows no badge while expanded or without surfaces", () => {
    expect(collapsedAttachmentBadge(2, false)).toBeNull();
    expect(collapsedAttachmentBadge(0, true)).toBeNull();
  });

  it("shows a bare globe for one surface and a count for more", () => {
    expect(collapsedAttachmentBadge(1, true)).toEqual({ count: null, label: "1 surface" });
    expect(collapsedAttachmentBadge(3, true)).toEqual({ count: 3, label: "3 surfaces" });
  });
});

describe("attachment block model", () => {
  it("hides rows and the tether when the owner is collapsed", () => {
    expect(attachmentBlockModel(["a", "b"], true, false)).toEqual({ rows: [], badge: { count: 2, label: "2 surfaces" }, tethered: false });
  });

  it("tethers expanded rows in the full sidebar", () => {
    expect(attachmentBlockModel(["a"], false, false)).toEqual({ rows: ["a"], badge: null, tethered: true });
  });

  it("keeps rows but drops the tether in the narrow sidebar", () => {
    expect(attachmentBlockModel(["a"], false, true)).toEqual({ rows: ["a"], badge: null, tethered: false });
  });
});
