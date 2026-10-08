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

    const toggle = row(host, owner.id)?.querySelector<HTMLButtonElement>('[data-testid="attachment-toggle"]');
    assert(toggle?.getAttribute("aria-label") === "Hide 1 surface", "owner row offers to fold its surface");
    await act(async () => toggle.click());
    assert(!row(host, surface.id), "collapsing the owner hides its surface");
    assert(row(host, owner.id)?.querySelector('[data-testid="attachment-toggle"]')?.getAttribute("data-collapsed") === "true", "collapsed owner keeps the globe badge");
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

void run();
