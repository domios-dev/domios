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

finish();
