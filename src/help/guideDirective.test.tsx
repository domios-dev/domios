import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { AppConfig } from "../lib/types";
import { GUIDE_SECTIONS, guideRequest, type GuideRequest } from "./guideNavigation";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

const testWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "HTMLSelectElement", "SVGElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, {
    value: key === "window" ? testWindow : testWindow[key],
    configurable: true,
  });
}
testWindow.matchMedia = (() => ({
  matches: false, media: "", onchange: null,
  addEventListener: () => {}, removeEventListener: () => {},
  addListener: () => {}, removeListener: () => {}, dispatchEvent: () => false,
})) as unknown as typeof testWindow.matchMedia;
const scrolled: string[] = [];
testWindow.HTMLElement.prototype.scrollIntoView = function () {
  const id = this.getAttribute("data-help-section");
  if (id) scrolled.push(id);
};

(async () => {
  const { __setInvokeForTest } = await import("../lib/bridge");
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "list_themes") return [] as T;
    if (command === "get_themes_dir") return "" as T;
    if (command === "get_autostart_enabled") return false as T;
    throw new Error(`unexpected invoke: ${command}`);
  });
  const { SettingsPanel } = await import("../components/Settings/SettingsPanel");
  const { RichMarkdown } = await import("../../packages/chat-ui/src/MarkdownBody");
  const { GuideNavigationProvider, parseGuideDirective } = await import("../../packages/chat-ui/src/GuideDirective");
  const config = { keybindings: {} } as AppConfig;

  function Harness({ text, localArtifacts = true }: { text: string; localArtifacts?: boolean }) {
    const [request, setRequest] = useState<GuideRequest>();
    const [open, setOpen] = useState(false);
    return (
      <GuideNavigationProvider sections={GUIDE_SECTIONS} onOpen={(id) => {
        setRequest(guideRequest(id));
        setOpen(true);
      }}>
        <RichMarkdown text={text} localArtifacts={localArtifacts} />
        {open && (
          <SettingsPanel
            config={config}
            onUpdateConfig={async () => {}}
            onClose={() => setOpen(false)}
            initialSection="guide"
            guideRequest={request}
          />
        )}
      </GuideNavigationProvider>
    );
  }

  const tests: Array<[string, () => void | Promise<void>]> = [];
  function test(name: string, run: () => void | Promise<void>) {
    tests.push([name, run]);
  }

  test("guide_parser_accepts_only_a_standalone_directive", () => {
    assert(parseGuideDirective("@ds-guide supporter-agent") === "supporter-agent", "section id lost");
    assert(parseGuideDirective("\t@ds-guide memory \r") === "memory", "CR line or indentation lost");
    for (const source of ["prefix @ds-guide memory", "@ds-guide memory suffix", "@ds-guide\nmemory", "@ds-guide memory\nsecond-line", "@ds-guide", "`@ds-guide memory`", "> @ds-guide memory"]) {
      assert(parseGuideDirective(source) === null, `accepted non-standalone directive: ${source}`);
    }
  });

  async function withHarness(text: string, run: (host: HTMLDivElement) => Promise<void>, localArtifacts = true) {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    scrolled.length = 0;
    try {
      await act(async () => root.render(<Harness text={text} localArtifacts={localArtifacts} />));
      await run(host);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  }

  function assertDestination(host: HTMLElement, id: string) {
    const page = host.querySelector('[data-settings-page="guide"]');
    assert(page, "Settings Guide did not open");
    const section = page.querySelector(`[data-help-section-id="${id}"]`);
    const heading = section?.querySelector("h3");
    assert(section && heading, "Guide destination missing");
    assert(document.activeElement === heading, "focus did not reach destination heading");
    assert(section.getAttribute("aria-labelledby") === heading.id, "section lost its accessible label");
    assert(scrolled.includes(id), "destination was not scrolled into view");
    assert(!(page.closest("[inert]") || page.closest('[aria-hidden="true"]')), "Guide destination hidden");
    const selector = page.querySelector('[data-testid="help-section-select"]') as HTMLSelectElement;
    assert(selector.value === id, "Guide section selection is stale");
  }

  test("guide_control_opens_settings_section_and_focuses_heading", async () => {
    await withHarness("@ds-guide supporter-agent", async (host) => {
      const control = host.querySelector('[data-testid="chat-ds-guide"] [data-block-action="open"]') as HTMLButtonElement;
      assert(control && control.type === "button" && !control.disabled, "Guide control inaccessible");
      assert((control.getAttribute("aria-label") ?? control.textContent)?.trim(), "Guide control has no accessible name");
      const metadata = GUIDE_SECTIONS.find((section) => section.id === "supporter-agent");
      assert(metadata && control.getAttribute("aria-label")?.includes(metadata.title), "section title did not reach control");
      await act(async () => control.click());
      assertDestination(host, "supporter-agent");

      const memoryNav = host.querySelector('[data-help-nav="memory"]') as HTMLButtonElement;
      await act(async () => memoryNav.click());
      const selector = host.querySelector('[data-testid="help-section-select"]') as HTMLSelectElement;
      assert(selector.value === "memory", "Guide navigation did not change section");
      const liveControl = host.querySelector('[data-testid="chat-ds-guide"] [data-block-action="open"]') as HTMLButtonElement;
      await act(async () => liveControl.click());
      assertDestination(host, "supporter-agent");
    });
  });

  test("unknown_guide_id_shows_notice_and_opens_first_section", async () => {
    await withHarness("@ds-guide unknown-section-3c2", async (host) => {
      assert(host.querySelector('[data-testid="chat-guide-notice"]'), "unknown id notice missing");
      const control = host.querySelector('[data-testid="chat-ds-guide"] [data-block-action="open"]') as HTMLButtonElement;
      await act(async () => control.click());
      assertDestination(host, GUIDE_SECTIONS[0].id);
    });
  });

  test("guide_directive_is_generic_across_sections", async () => {
    await withHarness("@ds-guide memory", async (host) => {
      const control = host.querySelector('[data-testid="chat-ds-guide"] [data-block-action="open"]') as HTMLButtonElement;
      await act(async () => control.click());
      assertDestination(host, "memory");
    });
  });

  test("inline_code_fences_quotes_and_file_previews_do_not_open_guide", async () => {
    for (const text of ["prefix @ds-guide memory", "```text\n@ds-guide memory\n```", "`@ds-guide memory`", "> @ds-guide memory"]) {
      await withHarness(text, async (host) => {
        assert(!host.querySelector('[data-testid="chat-ds-guide"]'), "non-standalone text became a Guide control");
      });
    }
    await withHarness("@ds-guide memory", async (host) => {
      assert(!host.querySelector('[data-testid="chat-ds-guide"]'), "file preview gained a Guide control");
    }, false);
  });

  let failed = 0;
  for (const [name, run] of tests) {
    try {
      await run();
      console.log(`  ✓ ${name}`);
    } catch (error) {
      failed++;
      console.error(`  ✗ ${name}: ${error instanceof Error ? error.message : error}`);
    }
  }
  __setInvokeForTest(null);
  console.log(`help/guideDirective tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  process.exit(failed === 0 ? 0 : 1);
})();
