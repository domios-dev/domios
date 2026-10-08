// Run: npx tsx src/components/Sidebar/TabItem.surface.test.tsx

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, finish, it } from "../../test/standaloneTest";
import { tabRowLabel } from "../../lib/surfaceTab";
import type { SurfaceTabBacking, Tab } from "../../lib/types";
import { TabItem } from "./TabItem";

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

finish();
