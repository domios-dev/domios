// Run: npx tsx src/components/Sidebar/TabItem.surface.test.tsx

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, finish, it } from "../../test/standaloneTest";
import { tabRowLabel } from "../../lib/surfaceTab";
import type { SurfaceTabBacking, Tab } from "../../lib/types";
import { TabItem, type AttachmentToggle } from "./TabItem";

const backing: SurfaceTabBacking = {
  kind: "surface",
  plugin_id: "dash",
  surface_id: "board",
  instance_id: "inst-1",
  owner_label: true,
};
const surfaceTab: Tab = { id: "s", title: "Board", tab_type: "Terminal", pty_id: null, cwd: null, backing, parent_tab_id: "owner" };

function titleText(ownerTitle: string | null): string | null {
  const html = renderToStaticMarkup(
    <TabItem
      tab={surfaceTab}
      index={0}
      isActive={false}
      onClick={() => undefined}
      onClose={() => undefined}
      canClose
      workspaceId="workspace"
      ownerTitle={ownerTitle}
    />,
  );
  const node = html.match(/<span data-testid="tab-title"[^>]*>([^<]*)<\/span>/)?.[1] ?? null;
  return node?.replace(/&#x27;|&#39;/g, "'") ?? null;
}

describe("surface tab row label", () => {
  it("shows the label the surface model composes from the owner's title", () => {
    expect(titleText("Jev")).toBe(tabRowLabel(surfaceTab, "Board", "Jev"));
  });

  it("falls back to the model's title-only label without an owner", () => {
    expect(titleText(null)).toBe(tabRowLabel(surfaceTab, "Board", null));
  });
});

const ownerTab: Tab = { id: "owner", title: "Snake", tab_type: "Terminal", pty_id: null, cwd: null, description: "Hunting bugs" };

function markup(tab: Tab, extra: { variant?: "attachment"; insetPx?: number; attachmentToggle?: AttachmentToggle | null }): string {
  return renderToStaticMarkup(
    <TabItem tab={tab} index={0} isActive={false} onClick={() => undefined} onClose={() => undefined} canClose workspaceId="workspace" {...extra} />,
  );
}

describe("surface attachment rows", () => {
  it("renders a compact row inset one avatar in from its owner", () => {
    const html = markup(surfaceTab, { variant: "attachment", insetPx: 30 });
    expect(html).toContain('data-variant="attachment"');
    expect(html).toContain("padding-left:30px");
    expect(html).toContain('data-testid="tab-surface-icon"');
  });

  it("keeps the subtitle stack off attachment rows", () => {
    const described = { ...surfaceTab, description: "Reading docs" };
    expect(markup(described, { variant: "attachment" })).not.toContain("tab-description-subtitle");
    expect(markup(described, {})).toContain("tab-description-subtitle");
  });

  it("badges a collapsed owner with a globe and a count only for several surfaces", () => {
    const toggle = (count: number, collapsed: boolean): AttachmentToggle => ({ count, collapsed, onToggle: () => undefined });
    const several = markup(ownerTab, { attachmentToggle: toggle(3, true) });
    expect(several).toContain('aria-label="Show 3 surfaces"');
    expect(several).toContain('data-testid="attachment-toggle-count">3<');
    const single = markup(ownerTab, { attachmentToggle: toggle(1, true) });
    expect(single).toContain('aria-label="Show 1 surface"');
    expect(single).not.toContain("attachment-toggle-count");
    const expanded = markup(ownerTab, { attachmentToggle: toggle(3, false) });
    expect(expanded).toContain('aria-label="Hide 3 surfaces"');
    expect(expanded).not.toContain("attachment-toggle-count");
    expect(markup(ownerTab, {})).not.toContain("attachment-toggle");
  });
});

finish();
