import { act, assert, createSuite, renderSidebar, setupDom } from "./SidebarWorkspaceTabs.testkit";
import { tab, TEAM } from "./SidebarWorkspaceTabs.fixtures.testdata";
import type { SurfaceTabBacking } from "../../lib/types";
import { attachmentCollapseStore, attachmentInset, ownerRowInset } from "./surfaceAttachmentModel";

setupDom();
const { test, run } = createSuite("workspace surface attachments");

const backing: SurfaceTabBacking = { kind: "surface", plugin_id: "browser", surface_id: "page", instance_id: "i", owner_label: true };

function row(host: Element, id: string): HTMLElement | null {
  return host.querySelector<HTMLElement>(`[data-tab-id="${id}"]`);
}

test("a loose owner's surface tucks under it as an attachment, not a tree child", async () => {
  attachmentCollapseStore.reset();
  const owner = tab({ id: "snake", title: "Snake", agent_type: "claude" });
  const surface = tab({ id: "snake-browser", title: "Browser", backing, parent_tab_id: "snake" });
  const child = tab({ id: "helper", title: "Helper", parent_tab_id: "snake" });
  const { host, root } = await renderSidebar({ tabs: { [owner.id]: owner, [surface.id]: surface, [child.id]: child } });
  try {
    const attachment = row(host, surface.id);
    assert(attachment?.getAttribute("data-variant") === "attachment", "surface renders as a compact attachment row");
    assert(attachment.closest('[data-testid="surface-owner-block"]')?.getAttribute("data-owner-id") === "snake", "attachment sits inside its owner's block");
    assert(row(host, child.id)?.getAttribute("data-variant") == null, "an ordinary child keeps the tree row");
    assert(!row(host, child.id)?.closest('[data-testid="surface-attachments"]'), "an ordinary child is not an attachment");
    assert(host.querySelectorAll('[data-tab-id="snake-browser"]').length === 1, "the surface renders once");

    assert(!row(host, owner.id)?.querySelector('[data-testid="attachment-toggle"]'), "surfaces cannot be hidden from the owner row");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    attachmentCollapseStore.reset();
  }
});

test("a Worker's surface tucks under that Worker at the Worker's indent plus one avatar", async () => {
  attachmentCollapseStore.reset();
  const orch = tab({ id: "orch", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id });
  const worker = tab({ id: "worker", role: "Worker", parent_tab_id: "orch", team_id: TEAM.id });
  const surface = tab({ id: "worker-browser", title: "Browser", backing, parent_tab_id: "worker", team_id: TEAM.id });
  const { host, root } = await renderSidebar({ tabs: { [orch.id]: orch, [worker.id]: worker, [surface.id]: surface } });
  try {
    const attachment = row(host, surface.id);
    assert(attachment?.closest('[data-testid="surface-owner-block"]')?.getAttribute("data-owner-id") === "worker", "Worker's surface lives in the Worker's block");
    const inset = attachmentInset(ownerRowInset("team", false));
    assert(attachment.style.paddingLeft === `${inset}px`, `attachment inset ${attachment.style.paddingLeft} is the Worker's indent plus one avatar`);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("an owner promoted into a Team takes its team-less surface into the Team card", async () => {
  attachmentCollapseStore.reset();
  const owner = tab({ id: "snake", title: "Snake", agent_type: "claude", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id });
  const surface = tab({ id: "snake-browser", title: "Browser", backing, parent_tab_id: "snake", team_id: null });
  const loose = tab({ id: "shell", title: "Shell" });
  const { host, root } = await renderSidebar({ tabs: { [surface.id]: surface, [loose.id]: loose, [owner.id]: owner } });
  try {
    assert(host.querySelectorAll('[data-tab-id="snake-browser"]').length === 1, "the surface renders once");
    const attachment = row(host, surface.id);
    assert(attachment?.getAttribute("data-variant") === "attachment", "surface renders as an attachment");
    assert(attachment.closest('[data-testid="surface-owner-block"]')?.getAttribute("data-owner-id") === "snake", "surface sits in its owner's block");
    assert(attachment.closest('[data-testid="team-unit"]'), "surface follows its owner into the Team card");
    assert(!attachment.closest(".sidebar-tab-tree-row"), "surface is not in the loose list");
    assert(row(host, loose.id)?.closest(".sidebar-tab-tree-row"), "unrelated loose tabs stay loose");
    const inset = attachmentInset(ownerRowInset("team", false));
    assert(attachment.style.paddingLeft === `${inset}px`, "surface takes the Team card's indent");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("a team-less surface follows a Worker owner into the Team card", async () => {
  attachmentCollapseStore.reset();
  const orch = tab({ id: "orch", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id });
  const worker = tab({ id: "worker", role: "Worker", parent_tab_id: "orch", team_id: TEAM.id });
  const surface = tab({ id: "worker-browser", title: "Browser", backing, parent_tab_id: "worker", team_id: null });
  const { host, root } = await renderSidebar({ tabs: { [orch.id]: orch, [worker.id]: worker, [surface.id]: surface } });
  try {
    const attachment = row(host, surface.id);
    assert(attachment?.closest('[data-testid="surface-owner-block"]')?.getAttribute("data-owner-id") === "worker", "surface sits in the Worker's block");
    assert(attachment.closest('[data-testid="team-unit"]'), "surface renders inside the Team card");
    assert(!attachment.closest(".sidebar-tab-tree-row"), "surface is not in the loose list");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("a surface whose owner is not in the sidebar falls back to a plain row", async () => {
  attachmentCollapseStore.reset();
  const surface = tab({ id: "orphan-browser", title: "Browser", backing, parent_tab_id: "elsewhere" });
  const other = tab({ id: "shell", title: "Shell" });
  const { host, root } = await renderSidebar({ tabs: { [surface.id]: surface, [other.id]: other } });
  try {
    const plain = row(host, surface.id);
    assert(plain, "orphaned surface still renders");
    assert(plain.getAttribute("data-variant") == null, "orphaned surface is a plain row");
    assert(!plain.closest('[data-testid="surface-owner-block"]'), "orphaned surface hangs under no owner");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

void run();
