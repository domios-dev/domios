import { renderToStaticMarkup } from "react-dom/server";
import { expect, finish, it } from "../../test/standaloneTest";
import type { Tab } from "../../lib/types";
import { TabItem } from "./TabItem";

function render(tab: Tab): string {
  return renderToStaticMarkup(
    <TabItem tab={tab} index={0} isActive={false} onClick={() => undefined} onClose={() => undefined}
      canClose workspaceId="workspace" />,
  );
}

function tab(overrides: Partial<Tab> = {}): Tab {
  return { id: "tab", pty_id: null, tab_type: "Terminal", title: "Build", cwd: null, ...overrides };
}

it("wires the selected type label and optional description into the sidebar row", () => {
  const described = render(tab({ role: "Manager", role_profile: "orchestrator", description: "Compile shaders" }));
  expect(described).toContain('data-testid="tab-title"');
  expect(described).toContain('data-testid="tab-description-subtitle"');
  expect(described).toContain("Compile shaders");
  expect(described).toContain('data-role-badge="ORCHESTRATOR"');

  const bare = render(tab({ role: "Manager", role_profile: "orchestrator" }));
  expect(bare).not.toContain('data-testid="tab-description-subtitle"');
  expect(bare).toContain('data-role-badge="ORCHESTRATOR"');
});

it("shows a Team-registered role label as declared on managers and custom workers", () => {
  const conductor = render(tab({ role: "Manager", role_profile: "orchestrator", role_label: "Conductor" }));
  expect(conductor).toContain('data-role-badge="Conductor"');
  expect(conductor).toContain(">Conductor</span>");
  expect(conductor).not.toContain("Orchestrator");

  const scout = render(tab({ role: "Worker", role_profile: "scout", role_label: "Field Scout" }));
  expect(scout).toContain(">Field Scout</span>");
});

it("reads a key-like worker label in sentence case on two lines, its description in the tooltip", () => {
  const scout = render(tab({ role: "Worker", role_profile: "field_scout", role_label: "Field Scout", description: "Scout probe" }));
  expect(scout).toContain(">Field scout</span>");
  expect(scout).not.toContain('data-testid="tab-description-subtitle"');
  expect(scout).toContain("Scout probe");

  const owner = render(tab({ role: "Worker", role_profile: "build_owner", role_label: "build_owner" }));
  expect(owner).toContain(">Build owner</span>");
});

finish();
