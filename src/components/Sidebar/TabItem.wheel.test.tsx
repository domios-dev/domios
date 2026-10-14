// Run: npx tsx src/components/Sidebar/TabItem.wheel.test.tsx

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, finish, it } from "../../test/standaloneTest";
import { TabItem } from "./TabItem";

function titleClasses(): string[] {
  const html = renderToStaticMarkup(
    <TabItem
      tab={{ id: "tab", pty_id: null, tab_type: "Terminal", title: "Nightly build", cwd: null }}
      index={0}
      isActive={false}
      onClick={() => undefined}
      onClose={() => undefined}
      canClose
      workspaceId="workspace"
    />,
  );
  const match = html.match(/<span data-testid="tab-title" class="([^"]*)"/);
  return match ? match[1].split(/\s+/) : [];
}

describe("tab label wheel passthrough", () => {
  it("clips the label instead of making it a scroll container", () => {
    const classes = titleClasses();
    expect(classes.includes("overflow-clip")).toBe(true);
    expect(classes.includes("truncate") || classes.includes("overflow-hidden")).toBe(false);
  });
});

finish();
