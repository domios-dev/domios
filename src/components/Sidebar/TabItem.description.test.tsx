// Run: npx tsx src/components/Sidebar/TabItem.description.test.tsx

import type { ComponentProps } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { Tab } from "../../lib/types";
import { TabItem } from "./TabItem";

const SUBTITLE_TESTID = 'data-testid="tab-description-subtitle"';
const SUBTITLE_NODE = /<span data-testid="tab-description-subtitle"[^>]*>[^<]*<\/span>/;
const ROOT_TOOLTIP = / title="[^"]*"/;

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

type Props = ComponentProps<typeof TabItem>;

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

/** The row tooltip already carried the description before the subtitle existed. */
function withoutRootTooltip(html: string): string {
  return html.replace(ROOT_TOOLTIP, ' title="[tooltip]"');
}

function withoutSubtitle(html: string): string {
  return withoutRootTooltip(html).replace(SUBTITLE_NODE, "");
}

function subtitleText(html: string): string | null {
  const node = html.match(SUBTITLE_NODE)?.[0];
  return node ? node.replace(/^<span[^>]*>/, "").replace(/<\/span>$/, "") : null;
}

const ORCHESTRATOR: Partial<Tab> = { role: "Manager", role_profile: "orchestrator" };

describe("tab description subtitle", () => {
  it("renders the description between the tab name and its role", () => {
    const html = render({ ...ORCHESTRATOR, description: "compiling shaders" });
    expect(subtitleText(html)).toBe("compiling shaders");
    const title = html.indexOf('data-testid="tab-title"');
    const subtitle = html.indexOf(SUBTITLE_TESTID);
    const role = html.indexOf('data-testid="tab-subline"');
    expect(title >= 0 && subtitle > title && role > subtitle).toBe(true);
  });

  it("adds nothing but the subtitle node to a row that already had a description tooltip", () => {
    const described = render({ ...ORCHESTRATOR, description: "compiling shaders" });
    const bare = render(ORCHESTRATOR);
    expect(bare).not.toContain(SUBTITLE_TESTID);
    expect(withoutSubtitle(described)).toBe(withoutRootTooltip(bare));
  });

  it("renders a row without a description exactly as a row with an empty one", () => {
    expect(render(ORCHESTRATOR)).toBe(render({ ...ORCHESTRATOR, description: "" }));
    expect(render({ ...ORCHESTRATOR, description: "   " })).not.toContain(SUBTITLE_TESTID);
  });

  it("keeps the full description in the row tooltip as the truncation escape hatch", () => {
    const description = "rebuilding the windows installer and waiting on the notarization queue";
    const html = render({ description });
    expect(html).toContain(`title="Nightly build\n${description}"`);
  });

  it("truncates an overlong description without adding or moving a layout node", () => {
    const short = "short";
    const long = `${"very long status text ".repeat(20)}end`;
    const shortHtml = render({ ...ORCHESTRATOR, description: short });
    const longHtml = render({ ...ORCHESTRATOR, description: long });
    expect(subtitleText(longHtml)).toBe(long);
    expect(withoutRootTooltip(longHtml).replace(long, short)).toBe(withoutRootTooltip(shortHtml));
    expect(longHtml.match(SUBTITLE_NODE)?.[0]).toContain("truncate");
    expect(longHtml).toContain('data-testid="tab-subline"');
  });

  it("renders the description on rows that carry no role badge", () => {
    const html = render({ description: "tailing logs" });
    expect(html).not.toContain('data-testid="tab-subline"');
    expect(subtitleText(html)).toBe("tailing logs");
  });

  it("keeps touch-mode row height classes with and without a description", () => {
    const described = render({ ...ORCHESTRATOR, description: "compiling shaders" }, { touchMode: true });
    const bare = render(ORCHESTRATOR, { touchMode: true });
    for (const html of [described, bare]) {
      expect(html).toContain("min-h-11");
      expect(html).toContain("sidebar-tab-touch");
    }
    expect(subtitleText(described)).toBe("compiling shaders");
    expect(withoutSubtitle(described)).toBe(withoutRootTooltip(bare));
  });

  it("keeps active and dimmed rows structurally unchanged apart from the subtitle", () => {
    for (const isActive of [true, false]) {
      const described = render({ ...ORCHESTRATOR, description: "compiling shaders" }, { isActive });
      const bare = render(ORCHESTRATOR, { isActive });
      expect(bare).toContain(isActive ? "text-[var(--ct-fg)]" : "sidebar-tab-dimmed");
      expect(withoutSubtitle(described)).toBe(withoutRootTooltip(bare));
    }
  });

  it("leaves a collapsed group head on its summary line", () => {
    const head = { groupCollapsible: true, collapsed: true, collapsedSummary: "1 manager · 2 workers" };
    const described = render({ ...ORCHESTRATOR, description: "compiling shaders" }, head);
    const bare = render(ORCHESTRATOR, head);
    expect(described).toContain('data-testid="group-collapsed-summary"');
    expect(described).not.toContain(SUBTITLE_TESTID);
    expect(withoutRootTooltip(described)).toBe(withoutRootTooltip(bare));
  });
});

finish();
