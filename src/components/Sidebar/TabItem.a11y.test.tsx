// Run: npx tsx src/components/Sidebar/TabItem.a11y.test.tsx

import type { ComponentProps } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { Tab } from "../../lib/types";
import { TabItem } from "./TabItem";

type Props = ComponentProps<typeof TabItem>;

function tab(overrides: Partial<Tab> = {}): Tab {
  return {
    id: "tab",
    pty_id: null,
    tab_type: "Terminal",
    title: "Nightly build",
    cwd: null,
    ...overrides,
  };
}

function render(overrides: Partial<Tab> = {}, props: Partial<Props> = {}): string {
  return renderToStaticMarkup(
    <TabItem
      tab={tab(overrides)}
      index={0}
      isActive={false}
      onClick={() => undefined}
      onClose={() => undefined}
      canClose
      workspaceId="workspace"
      {...props}
    />,
  );
}

/** The accessible name of the row group, as the DOM exposes it. */
function rowName(html: string): string | null {
  return html.match(/<div[^>]*role="group"[^>]*aria-label="([^"]*)"/)?.[1] ?? null;
}

function orderOf(html: string, needles: string[]): number[] {
  return needles.map((needle) => html.indexOf(needle));
}

describe("tab row accessibility surface", () => {
  it("names the row from the title it was given", () => {
    for (const title of ["Nightly build", "release cut 1.10.0", "shader bake"]) {
      expect(rowName(render({ title }))).toBe(title);
    }
  });

  it("keeps the row name out of the position, so a re-sort cannot rename it", () => {
    const first = rowName(render({ title: "shader bake" }, { index: 0 }));
    const later = rowName(render({ title: "shader bake" }, { index: 7 }));
    expect(first).toBe("shader bake");
    expect(later).toBe(first);
  });

  it("keeps the row name off the live description, so status churn cannot rename it", () => {
    const idle = rowName(render({ title: "shader bake" }));
    const busy = rowName(render({ title: "shader bake", description: "compiling shaders" }));
    const later = rowName(render({ title: "shader bake", description: "linking" }));
    expect(idle).toBe("shader bake");
    expect(busy).toBe(idle);
    expect(later).toBe(idle);
  });

  it("names the activation control and the row from the same title", () => {
    const html = render({ title: "release cut" });
    expect(rowName(html)).toBe("release cut");
    expect(html).toContain('aria-label="Open tab release cut"');
  });

  it("names the Telegram indicator from the tab it belongs to", () => {
    const html = render({ title: "release cut", connected_services: ["telegram"] });
    expect(html).toContain('aria-label="release cut is connected to Telegram"');
    expect(render({ title: "release cut" })).not.toContain("connected to Telegram");
  });

  it("exposes the pending-prompt indicator as an image rather than a bare dot", () => {
    const html = render({
      title: "release cut",
      pending_prompt: { question: "Overwrite the tag?", options: [] },
    });
    expect(html).toContain('title="Overwrite the tag?"');
    expect(html).toContain('role="img"');
  });

  it("puts the collapse chevron, the row activation and the row menu in visual order", () => {
    const html = render(
      { title: "release cut" },
      { groupCollapsible: true, collapsed: false, onToggleCollapse: () => undefined },
    );
    const [chevron, activation, menu] = orderOf(html, [
      'data-testid="group-collapse-toggle"',
      "sidebar-tab-activation",
      'aria-label="Tab actions"',
    ]);
    expect(chevron >= 0 && activation > chevron && menu > activation).toBe(true);
  });

  it("keeps the activation control ahead of the row menu without a chevron", () => {
    const html = render({ title: "release cut" });
    const [activation, menu] = orderOf(html, ["sidebar-tab-activation", 'aria-label="Tab actions"']);
    expect(activation >= 0 && menu > activation).toBe(true);
  });
});

finish();
