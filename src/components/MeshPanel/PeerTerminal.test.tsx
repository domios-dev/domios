// Run: npx tsx src/components/MeshPanel/PeerTerminal.test.tsx

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { PeerTerminal } from "./PeerTerminal";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://127.0.0.1/peer-terminal" });
  for (const key of ["window", "document", "HTMLElement", "HTMLButtonElement"]) {
    Object.defineProperty(globalThis, key, {
      value: (window as unknown as Record<string, unknown>)[key],
      configurable: true,
    });
  }
}

async function render(props: Parameters<typeof PeerTerminal>[0]): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<PeerTerminal {...props} />);
  });
  return { host, root };
}

const base = {
  name: "rollacode-bot",
  ip: "100.64.0.2",
  isPaired: true,
  accessLevel: "full" as const,
  services: [],
};

test("peer action appears only for a verified URL", async () => {
  const { host, root } = await render({ ...base, webUrl: "https://bot.tail.ts.net/" });
  const btn = host.querySelector('[data-testid="peer-web-ui-open"]');
  assert(btn, "peer-web-ui-open present with verified url");
  assert(btn?.getAttribute("data-peer-web-url") === "https://bot.tail.ts.net/", "carries url");
  await act(async () => root.unmount());
});

test("peer action hidden when no verified URL", async () => {
  const { host, root } = await render({ ...base, webUrl: null });
  assert(!host.querySelector('[data-testid="peer-web-ui-open"]'), "no action without url");
  await act(async () => root.unmount());
});

test("clicking peer action opens the verified url and does not select the card", async () => {
  let opened: string | null = null;
  let cardClicked = false;
  const { host, root } = await render({
    ...base,
    webUrl: "https://bot.tail.ts.net/",
    onOpenWebUi: (u) => { opened = u; },
    onClick: () => { cardClicked = true; },
  });
  const btn = host.querySelector('[data-testid="peer-web-ui-open"]') as HTMLButtonElement;
  await act(async () => btn.click());
  assert(opened === "https://bot.tail.ts.net/", "opened verified url");
  assert(!cardClicked, "click did not bubble to card select");
  await act(async () => root.unmount());
});

async function main() {
  setupDom();
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.stack : e}`);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} PeerTerminal tests passed`);
}

main();
